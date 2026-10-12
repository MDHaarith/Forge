#![cfg(unix)]
mod common;
#[path = "common/claude.rs"]
mod support;

use forge_server::{
    model::{default_model, make_model, Model, ModelRequest},
    providers::{available_providers, model_choices, provider_label, CLAUDE_MODELS},
};
use serde_json::{json, Value};
use std::time::Duration;
use support::{events, FakeServer, SUCCESS, WAIT};

#[tokio::test]
async fn cli_stream_args_stdin_resume_and_persisted_history() {
    let server = FakeServer::start(SUCCESS).await;
    let streamed = events(server.chat("hello CLI").await).await;
    // User events preserve the existing UI/history contract; Claude supplies text then done.
    assert_eq!(
        streamed,
        [
            json!({"type":"user","text":"hello CLI"}),
            json!({"type":"text","text":"hello from claude"}),
            json!({"type":"done"})
        ]
    );
    assert_eq!(server.history().await["events"], json!(streamed));
    let saved: Value = serde_json::from_slice(
        &std::fs::read(server.settings.home.join(".state/test.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["claude_session"], "sess-1");
    assert_eq!(saved["messages"], json!([]));
    assert_eq!(saved["events"], json!(streamed));
    let args = std::fs::read_to_string(server.dir().join("args")).unwrap();
    let args: Vec<_> = args.lines().collect();
    for expected in ["-p", "--verbose", "--strict-mcp-config"] {
        assert!(args.contains(&expected));
    }
    for (flag, value) in [
        ("--tools", ""),
        ("--setting-sources", ""),
        ("--output-format", "stream-json"),
        ("--model", CLAUDE_MODELS[0]),
        ("--allowedTools", "mcp__forge"),
    ] {
        let index = args.iter().position(|arg| *arg == flag).unwrap();
        assert_eq!(args[index + 1], value);
    }
    let index = args.iter().position(|arg| *arg == "--mcp-config").unwrap();
    let config: Value = serde_json::from_str(args[index + 1]).unwrap();
    assert_eq!(
        config,
        json!({"mcpServers":{"forge":{"type":"http","url":format!("{}/mcp/test", server.base)}}})
    );
    assert_eq!(
        std::fs::read_to_string(server.dir().join("stdin")).unwrap(),
        "hello CLI"
    );
    assert_eq!(
        std::fs::read_to_string(server.dir().join("cwd"))
            .unwrap()
            .trim(),
        server.dir().to_str().unwrap()
    );
    events(server.chat("again").await).await;
    let args = std::fs::read_to_string(server.dir().join("args")).unwrap();
    assert!(args.contains("--resume\nsess-1\n"));
    assert_eq!(
        server.history().await["events"].as_array().unwrap().len(),
        6
    );
    // A new server instance must load the saved CLI session as well as its events.
    let selected = default_model(&server.settings);
    let projects = forge_server::projects::Projects::new(server.settings.clone(), selected);
    assert_eq!(
        *projects
            .session("test")
            .await
            .unwrap()
            .claude_session
            .lock()
            .unwrap(),
        Some("sess-1".into())
    );
    assert!(server.temp.path.exists());
}

#[tokio::test]
async fn stderr_failure_emits_error_then_done_and_disables_mcp() {
    let server = FakeServer::start("printf 'boom\\n' >&2; exit 1\n").await;
    let streamed = events(server.chat("hi").await).await;
    assert_eq!(streamed.len(), 3);
    assert!(streamed[1]["message"].as_str().unwrap().contains("boom"));
    assert_eq!(streamed[2], json!({"type":"done"}));
    assert_eq!(server.history().await["running"], false);
    let rpc: Value = server
        .post(
            "/mcp/test",
            json!({"id":1,"method":"tools/call","params":{"name":"write_file"}}),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(rpc["error"]["code"], -32000);
}

#[tokio::test]
async fn stderr_errors_keep_only_the_last_twenty_lines() {
    let script =
        "i=1\nwhile [ \"$i\" -le 40 ]; do echo \"line-$i\" >&2; i=$((i + 1)); done\nexit 1\n";
    let server = FakeServer::start(script).await;
    let streamed = events(server.chat("tail").await).await;
    let tail = streamed[1]["message"].as_str().unwrap();
    assert_eq!(tail.lines().count(), 20);
    assert!(tail.starts_with("line-21\n"));
    assert!(tail.ends_with("line-40"));
    assert_eq!(streamed[2]["type"], "done");
}

#[tokio::test]
async fn startup_resolves_override_or_path_and_cli_inherits_login_without_claudecode() {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    // Environment changes belong to a child process, never parallel test threads.
    for override_path in [true, false] {
        let script = format!("printf '%s' \"$FORGE_TEST_LOGIN\" > login\nprintf '%s' \"${{CLAUDECODE-unset}}\" > claudecode\n{SUCCESS}");
        let server = FakeServer::start(&script).await;
        let bin = server.temp.path.join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::copy(
            server.settings.claude_program.as_ref().unwrap(),
            bin.join("claude"),
        )
        .unwrap();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_forge-server"));
        command
            .env_clear()
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("HOME", &server.temp.path)
            .env("PORT", "0")
            .env("FORGE_HOME", &server.settings.home)
            .env("FORGE_APP_DIR", &server.settings.app_dir)
            .env("FORGE_ENV_FILE", server.temp.path.join("no-env-file"))
            .env("FORGE_EXIT_WITH_STDIN", "1")
            .env("FORGE_TEST_LOGIN", "user-login")
            .env("CLAUDECODE", "nested")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if override_path {
            command.env(
                "FORGE_CLAUDE",
                server.settings.claude_program.as_ref().unwrap(),
            );
        }
        let mut child = command.spawn().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let address = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let base = address.strip_prefix("Forge is running at ").unwrap();
        let response = reqwest::Client::new()
            .post(format!("{base}/api/projects/test/chat"))
            .json(&json!({"message":"startup"}))
            .send()
            .await
            .unwrap();
        let streamed = events(response).await;
        assert_eq!(streamed[1]["text"], "hello from claude");
        assert_eq!(
            std::fs::read_to_string(server.dir().join("login")).unwrap(),
            "user-login"
        );
        assert_eq!(
            std::fs::read_to_string(server.dir().join("claudecode")).unwrap(),
            "unset"
        );
        let args = std::fs::read_to_string(server.dir().join("args")).unwrap();
        assert!(args.contains(&format!("{base}/mcp/test")));
        drop(child.stdin.take());
        assert!(tokio::time::timeout(Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success());
    }
}

#[tokio::test]
async fn result_errors_do_not_duplicate_exit_errors_and_fall_back_to_subtype() {
    for (line, expected) in [
        (
            r#"{"type":"result","is_error":true,"result":"declined"}"#,
            "declined",
        ),
        (
            r#"{"type":"result","is_error":true,"subtype":"error_during_execution"}"#,
            "error_during_execution",
        ),
    ] {
        let server =
            FakeServer::start(&format!("printf '%s\\n' '{line}'\necho boom >&2\nexit 1\n")).await;
        let streamed = events(server.chat("hi").await).await;
        assert_eq!(
            streamed,
            [
                json!({"type":"user","text":"hi"}),
                json!({"type":"error","message":expected}),
                json!({"type":"done"})
            ]
        );
    }
}

#[tokio::test]
async fn missing_program_emits_start_error_then_done() {
    let server = FakeServer::start(SUCCESS).await;
    // The selection is made at startup, then the executable disappears before spawn.
    events(server.chat("first").await).await;
    std::fs::remove_file(server.settings.claude_program.as_ref().unwrap()).unwrap();
    let streamed = events(server.chat("hi").await).await;
    assert!(streamed[1]["message"]
        .as_str()
        .unwrap()
        .starts_with("Claude Code could not start: "));
    assert_eq!(streamed[2]["type"], "done");
    assert_eq!(server.history().await["running"], false);
}

#[tokio::test]
async fn stale_resume_retries_once_for_stderr_and_result_failures() {
    for failure in [
        "echo 'No conversation found' >&2",
        r#"printf '%s\n' '{"type":"result","is_error":true,"result":"missing sess-1"}'"#,
    ] {
        let script = format!("for arg in \"$@\"; do\nif [ \"$arg\" = --resume ]; then\n{failure}\nexit 1\nfi\ndone\n{SUCCESS}");
        let server = FakeServer::start(&script).await;
        events(server.chat("first").await).await;
        let streamed = events(server.chat("retry").await).await;
        assert_eq!(
            streamed,
            [
                json!({"type":"user","text":"retry"}),
                json!({"type":"text","text":"hello from claude"}),
                json!({"type":"done"})
            ]
        );
        let args = std::fs::read_to_string(server.dir().join("args")).unwrap();
        assert_eq!(args.lines().filter(|arg| *arg == "-p").count(), 3);
        assert_eq!(args.lines().filter(|arg| *arg == "--resume").count(), 1);
    }
}

#[tokio::test]
async fn failed_retry_clears_stale_session_and_stops_after_two_attempts() {
    let script = "for arg in \"$@\"; do\nif [ \"$arg\" = --resume ]; then echo 'No conversation found' >&2; exit 1; fi\ndone\necho boom >&2; exit 1\n";
    let server = FakeServer::start(script).await;
    std::fs::create_dir_all(server.settings.home.join(".state")).unwrap();
    std::fs::write(
        server.settings.home.join(".state/test.json"),
        r#"{"messages":[],"events":[],"claude_session":"sess-1"}"#,
    )
    .unwrap();
    let streamed = events(server.chat("retry").await).await;
    assert_eq!(streamed[1], json!({"type":"error","message":"boom"}));
    assert_eq!(streamed[2], json!({"type":"done"}));
    let args = std::fs::read_to_string(server.dir().join("args")).unwrap();
    assert_eq!(args.lines().filter(|arg| *arg == "-p").count(), 2);
    let saved: Value = serde_json::from_slice(
        &std::fs::read(server.settings.home.join(".state/test.json")).unwrap(),
    )
    .unwrap();
    assert!(saved.get("claude_session").is_none());
}

#[tokio::test]
async fn ignores_non_json_tool_stream_and_unknown_events_and_drains_large_pipes() {
    let script = r#"echo not-json
printf '%s\n' '{"type":"rate_limit_event"}' '{"type":"unknown"}' '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"ignored","name":"mcp__forge__write_file"},{"type":"text","text":" "}]}}' '{"type":"user","message":{"content":[{"type":"tool_result"}]}}'
head -c 100000 /dev/zero | tr '\000' x >&2
printf '{"type":"assistant","message":{"content":[{"type":"text","text":"'
head -c 100000 /dev/zero | tr '\000' x
printf '"}]}}\n'
"#;
    let server = FakeServer::start(script).await;
    let streamed = events(server.chat(&"m".repeat(100000)).await).await;
    assert_eq!(streamed.len(), 3);
    assert_eq!(streamed[1]["text"].as_str().unwrap().len(), 100000);
    assert_eq!(streamed[2]["type"], "done");
    assert_eq!(
        std::fs::read(server.dir().join("stdin")).unwrap().len(),
        100000
    );
}

#[tokio::test]
async fn busy_is_set_before_response_and_disconnect_keeps_turn_and_save_alive() {
    let server = FakeServer::start(WAIT).await;
    let response = server.chat("save this").await;
    assert_eq!(
        server
            .post("/api/model", json!({"provider":"offline"}))
            .await
            .status(),
        409
    );
    assert_eq!(server.chat("again").await.status(), 409);
    server.wait_started().await;
    assert_eq!(server.history().await["running"], true);
    drop(response);
    std::fs::write(server.dir().join("go"), "").unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let history = server.history().await;
            if history["running"] == false
                && history["events"].as_array().unwrap().last().unwrap()["type"] == "done"
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let saved: Value = serde_json::from_slice(
        &std::fs::read(server.settings.home.join(".state/test.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["events"], server.history().await["events"]);
    assert_eq!(saved["claude_session"], "sess-1");
    assert_eq!(saved["messages"], json!([]));
}

#[tokio::test]
async fn provider_models_defaults_picker_and_cli_model_boundary() {
    let server = FakeServer::start(SUCCESS).await;
    let mut settings = server.settings.clone();
    assert_eq!(available_providers(&settings), ["claude", "offline"]);
    assert_eq!(provider_label("claude"), "Claude (Claude Code)");
    assert_eq!(model_choices("claude", &settings).await, CLAUDE_MODELS);
    assert_eq!(default_model(&settings).provider(), "claude");
    assert_eq!(default_model(&settings).name(), CLAUDE_MODELS[0]);
    settings.startup_model = Some("custom-claude".into());
    assert_eq!(default_model(&settings).name(), "custom-claude");
    settings.startup_provider = Some("unknown".into());
    assert_eq!(default_model(&settings).provider(), "claude");
    settings.startup_provider = Some("offline".into());
    assert_eq!(default_model(&settings).provider(), "offline");
    settings.startup_provider = Some("claude".into());
    settings.offline = true;
    assert_eq!(default_model(&settings).provider(), "offline");
    settings.claude_program = None;
    assert_eq!(available_providers(&settings), ["offline"]);
    settings.claude_program = Some(server.temp.path.join("missing"));
    assert_eq!(available_providers(&settings), ["offline"]);
    let request = ModelRequest {
        system: String::new(),
        tools: vec![],
        messages: vec![],
    };
    assert!(make_model("claude", None, &settings)
        .respond(&request)
        .await
        .unwrap_err()
        .contains("CLI"));
    let choices: Value = reqwest::Client::new()
        .get(format!("{}/api/models", server.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        choices["providers"][0],
        json!({"id":"claude","label":"Claude (Claude Code)","models":CLAUDE_MODELS})
    );
    for body in [
        json!({"provider":"missing"}),
        json!({"provider":"claude","model":1}),
    ] {
        assert_eq!(server.post("/api/model", body).await.status(), 400);
    }
    assert_eq!(
        server
            .post("/api/model", json!({"provider":"offline"}))
            .await
            .status(),
        200
    );
    let switched: Value = server
        .post(
            "/api/model",
            json!({"provider":"claude","model":"claude-sonnet-5-5"}),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        switched,
        json!({"provider":"claude","model":"claude-sonnet-5-5"})
    );
    events(server.chat("selected model").await).await;
    assert!(std::fs::read_to_string(server.dir().join("args"))
        .unwrap()
        .contains("--model\nclaude-sonnet-5-5\n"));
}
