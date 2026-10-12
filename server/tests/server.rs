mod common;
use common::TempDir;
use forge_server::{
    http::{router, AppState},
    model::{Model, ModelFuture, ModelReply, ModelRequest, OfflineModel, SwitchableModel},
    projects::Projects,
    Settings,
};
use serde_json::{json, Value};
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::Notify,
};

struct TestServer {
    temp: TempDir,
    base: String,
    state: AppState,
    task: tokio::task::JoinHandle<()>,
}
impl TestServer {
    async fn start(model: Arc<dyn Model>) -> Self {
        let temp = TempDir::new("http");
        let settings = Settings::new(temp.path.join("home"), temp.path.join("app"));
        let selected = Arc::new(SwitchableModel::new("offline", model));
        let state = AppState::new(
            Arc::new(Projects::new(settings.clone(), selected.clone())),
            selected,
            settings,
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = router(state.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            temp,
            base,
            state,
            task,
        }
    }
    async fn offline() -> Self {
        Self::start(Arc::new(OfflineModel)).await
    }
    async fn get(&self, path: &str) -> reqwest::Response {
        reqwest::Client::new()
            .get(format!("{}/api{path}", self.base))
            .send()
            .await
            .unwrap()
    }
    async fn post(&self, path: &str, body: Value) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("{}/api{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap()
    }
}
impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn events(text: &str) -> Vec<Value> {
    text.trim()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn status_lists_every_capability() {
    let server = TestServer::offline().await;
    let status: Value = server.get("/status").await.json().await.unwrap();
    assert_eq!(status["offline"], true);
    let tools = status["tools"].as_array().unwrap();
    let names = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    for n in ["write_file", "git_commit", "cad_model", "pcb_export_fab", "chip_synthesize"] {
        assert!(names.contains(&n), "{n}");
    }
    assert_eq!(
        tools.iter().find(|t| t["name"] == "cad_model").unwrap()["available"],
        true
    );
    assert_eq!(
        tools.iter().find(|t| t["name"] == "run_command").unwrap()["requires"],
        "FORGE_ALLOW_COMMANDS=1"
    );
    assert_eq!(
        tools.iter().find(|t| t["name"] == "run_command").unwrap()["available"],
        false
    );
    assert!(tools.iter().all(|t| t.get("available").is_some()));
}

#[tokio::test]
async fn offline_demo_builds_and_commits_enclosure_end_to_end() {
    let server = TestServer::offline().await;
    assert_eq!(
        server
            .post("/projects", json!({"name":"rover"}))
            .await
            .status(),
        201
    );
    assert_eq!(
        server
            .post("/projects", json!({"name":"rover"}))
            .await
            .status(),
        400
    );
    assert_eq!(
        server
            .post("/projects", json!({"name":"../evil"}))
            .await
            .status(),
        400
    );
    let res = server
        .post(
            "/projects/rover/chat",
            json!({"message":"Design an enclosure"}),
        )
        .await;
    assert_eq!(res.headers()["content-type"], "application/x-ndjson");
    let streamed = events(&res.text().await.unwrap());
    let calls = streamed
        .iter()
        .filter(|e| e["type"] == "tool_call")
        .map(|e| e["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(calls, ["cad_model", "git_commit"]);
    assert!(streamed
        .iter()
        .filter(|e| e["type"] == "tool_result")
        .all(|e| e["ok"] == true));
    assert_eq!(streamed.last().unwrap()["type"], "done");
    let files: Vec<String> = server
        .get("/projects/rover/files")
        .await
        .json()
        .await
        .unwrap();
    assert!(files.contains(&"models/demo-enclosure.stl".into()));
    let stl = server
        .get("/projects/rover/file?path=models/demo-enclosure.stl")
        .await
        .text()
        .await
        .unwrap();
    assert!(stl.starts_with("solid demo-enclosure"));
    let history: Value = server
        .get("/projects/rover/history")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(history["events"].as_array().unwrap().len(), streamed.len());
    let saved: Value = serde_json::from_slice(
        &std::fs::read(server.state.projects.home.join(".state/rover.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        saved["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "user",
            "assistant",
            "user",
            "assistant",
            "user",
            "assistant"
        ]
    );
    let json = std::fs::read_to_string(
        server
            .state
            .projects
            .home
            .join("rover/models/demo-enclosure.json"),
    )
    .unwrap();
    assert!(!json.contains(".0"));
    // Golden fingerprints from TS cad_model compare every byte without a large STL fixture.
    for (ext, length, expected) in [
        ("stl", 85815, 15860311762566082420u64),
        ("scad", 533, 752138743025860563u64),
        ("json", 1339, 10979603412810776451u64),
    ] {
        let bytes = std::fs::read(
            server
                .state
                .projects
                .home
                .join(format!("rover/models/demo-enclosure.{ext}")),
        )
        .unwrap();
        assert_eq!(bytes.len(), length);
        let hash = bytes.iter().fold(14695981039346656037u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(1099511628211)
        });
        assert_eq!(hash, expected, "TS output differs: {ext}");
    }
    let log = forge_server::tools::exec::run(
        "git",
        &["log".into(), "-1".into(), "--format=%s".into()],
        &server.state.projects.dir("rover").unwrap(),
        5000,
        &[],
    )
    .await;
    assert_eq!(log.stdout.trim(), "Add demo enclosure");
    assert_eq!(
        server.get("/projects").await.json::<Value>().await.unwrap(),
        json!(["rover"])
    );
}

#[tokio::test]
async fn rejects_foreign_hosts_non_json_and_path_escapes() {
    let server = TestServer::offline().await;
    server.state.projects.create("rover").await.unwrap();
    let client = reqwest::Client::new();
    let foreign = client
        .get(format!("{}/api/status", server.base))
        .header("host", "evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), 403);
    assert_eq!(
        foreign.json::<Value>().await.unwrap(),
        json!({"error":"forbidden host"})
    );
    let form = client
        .post(format!("{}/api/projects", server.base))
        .body("name=x")
        .send()
        .await
        .unwrap();
    assert_eq!(form.status(), 415);
    assert_eq!(
        form.json::<Value>().await.unwrap(),
        json!({"error":"expected application/json"})
    );
    assert_eq!(
        server
            .get("/projects/rover/file?path=../../etc/passwd")
            .await
            .status(),
        400
    );
    assert_eq!(server.get("/projects/nope/files").await.status(), 404);
    assert_eq!(
        server
            .get("/projects/rover/file?path=a/../.git/config")
            .await
            .status(),
        400
    );
    assert_eq!(
        server
            .get("/projects/rover/file?path=missing")
            .await
            .status(),
        404
    );
}

#[tokio::test]
async fn desktop_server_exits_when_stdin_closes_and_prints_actual_port() {
    let dir = TempDir::new("stdin");
    let envfile = dir.path.join("env");
    std::fs::write(
        &envfile,
        "PORT=4317\nFORGE_HOME=/env-file-home\nFORGE_OFFLINE=1\n",
    )
    .unwrap();
    let home = dir.path.join("home");
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_forge-server"))
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &dir.path)
        .env("PORT", "0")
        .env("FORGE_HOME", &home)
        .env("FORGE_APP_DIR", &dir.path)
        .env("FORGE_ENV_FILE", &envfile)
        .env("FORGE_EXIT_WITH_STDIN", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first = tokio::time::timeout(Duration::from_secs(20), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let prefix = "Forge is running at http://127.0.0.1:";
    assert!(first.starts_with(prefix));
    assert_ne!(
        first.strip_prefix(prefix).unwrap().parse::<u16>().unwrap(),
        0
    );
    assert_eq!(
        lines.next_line().await.unwrap().unwrap(),
        "Model: offline demo (Offline demo)"
    );
    assert_eq!(
        lines.next_line().await.unwrap().unwrap(),
        format!("Projects: {}", home.display())
    );
    drop(child.stdin.take());
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(20), child.wait())
            .await
            .unwrap()
            .unwrap()
            .code(),
        Some(0)
    );
}

#[tokio::test]
async fn model_picker_lists_providers_and_rejects_unknown_models() {
    let server = TestServer::offline().await;
    let state: Value = server.get("/models").await.json().await.unwrap();
    assert_eq!(state["provider"], "offline");
    assert!(state["providers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == "offline"));
    assert_eq!(
        server
            .post("/model", json!({"provider":"nope"}))
            .await
            .status(),
        400
    );
    assert_eq!(
        server
            .post("/model", json!({"provider":"offline","model":5}))
            .await
            .status(),
        400
    );
    assert_eq!(
        server
            .post("/model", json!({"provider":"offline"}))
            .await
            .status(),
        200
    );
}

struct BlockingModel {
    started: Notify,
    release: Notify,
}
impl Model for BlockingModel {
    fn name(&self) -> String {
        "blocking".into()
    }
    fn respond<'a>(&'a self, _: &'a ModelRequest) -> ModelFuture<'a> {
        Box::pin(async move {
            self.started.notify_one();
            self.release.notified().await;
            Ok(ModelReply {
                stop_reason: Some("end_turn".into()),
                content: vec![],
            })
        })
    }
}

#[tokio::test]
async fn model_switch_and_second_chat_are_refused_while_history_and_files_respond() {
    let blocking = Arc::new(BlockingModel {
        started: Notify::new(),
        release: Notify::new(),
    });
    let server = TestServer::start(blocking.clone()).await;
    server.state.projects.create("busy").await.unwrap();
    let mut chat = server
        .post("/projects/busy/chat", json!({"message":"hi"}))
        .await;
    assert_eq!(
        server
            .post("/model", json!({"provider":"offline"}))
            .await
            .status(),
        409
    );
    assert_eq!(
        server
            .post("/projects/busy/chat", json!({"message":"again"}))
            .await
            .status(),
        409
    );
    tokio::time::timeout(Duration::from_secs(5), blocking.started.notified())
        .await
        .unwrap();
    let chunk = tokio::time::timeout(Duration::from_secs(2), chat.chunk())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&chunk).contains("\"type\":\"user\""));
    let history =
        tokio::time::timeout(Duration::from_secs(2), server.get("/projects/busy/history"))
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
    assert_eq!(history["running"], true);
    assert_eq!(history["events"][0]["text"], "hi");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), server.get("/projects/busy/files"))
            .await
            .unwrap()
            .status(),
        200
    );
    blocking.release.notify_one();
    chat.text().await.unwrap();
    assert_eq!(
        server
            .post("/model", json!({"provider":"offline"}))
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn disconnected_client_does_not_cancel_turn_or_state_save() {
    let blocking = Arc::new(BlockingModel {
        started: Notify::new(),
        release: Notify::new(),
    });
    let server = TestServer::start(blocking.clone()).await;
    server.state.projects.create("busy").await.unwrap();
    let response = server
        .post("/projects/busy/chat", json!({"message":"save this"}))
        .await;
    tokio::time::timeout(Duration::from_secs(5), blocking.started.notified())
        .await
        .unwrap();
    drop(response);
    blocking.release.notify_one();
    let state_file = server.state.projects.home.join(".state/busy.json");
    let saved = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(bytes) = tokio::fs::read(&state_file).await {
                if let Ok(saved) = serde_json::from_slice::<Value>(&bytes) {
                    break saved;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        saved["events"].as_array().unwrap().last().unwrap()["type"],
        "done"
    );
    assert_eq!(saved["messages"][0]["content"], "save this");
    assert!(!server.state.projects.running());
}

#[tokio::test]
async fn loads_typescript_shaped_state_without_changing_history() {
    let server = TestServer::offline().await;
    server.state.projects.create("saved").await.unwrap();
    let document = json!({"messages":[{"role":"user","content":"old"},{"role":"assistant","content":[{"type":"thinking","thinking":"hmm","signature":"s"},{"type":"text","text":"old answer","citations":null}]}],
        "events":[{"type":"user","text":"old"},{"type":"text","text":"old answer"},{"type":"done"}]});
    tokio::fs::create_dir_all(server.state.projects.home.join(".state"))
        .await
        .unwrap();
    tokio::fs::write(
        server.state.projects.home.join(".state/saved.json"),
        document.to_string(),
    )
    .await
    .unwrap();
    let history: Value = server
        .get("/projects/saved/history")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(history["events"], document["events"]);
    assert_eq!(history["running"], false);
    let session = server.state.projects.session("saved").await.unwrap();
    assert_eq!(
        *session.agent.messages.lock().unwrap(),
        document["messages"].as_array().unwrap().clone()
    );
    server
        .post("/projects/saved/chat", json!({"message":"hi"}))
        .await
        .text()
        .await
        .unwrap();
    let data = std::fs::read(server.state.projects.home.join(".state/saved.json")).unwrap();
    let saved: Value = serde_json::from_slice(&data).unwrap();
    assert_eq!(saved["messages"][1], document["messages"][1]);
}

#[tokio::test]
async fn manual_routes_limits_json_errors_and_static_content_types() {
    let server = TestServer::offline().await;
    let client = reqwest::Client::new();
    let root = client.get(&server.base).send().await.unwrap();
    assert_eq!(root.status(), 503);
    assert_eq!(
        root.json::<Value>().await.unwrap()["error"],
        "the interface is not built yet: run `npm start`, or `npm run dev` for development"
    );
    for host in ["localhost:9", "[::1]:4317", "127.0.0.1"] {
        assert_eq!(
            client
                .get(format!("{}/api/projects", server.base))
                .header("host", host)
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
    }
    assert_eq!(server.get("/missing").await.status(), 404);
    assert_eq!(
        server
            .get("/projects/%FF/files")
            .await
            .json::<Value>()
            .await
            .unwrap(),
        json!({"error":"URI malformed"})
    );
    assert_eq!(server.get("/projects/%2e%2e/files").await.status(), 404);
    let empty = client
        .post(format!("{}/api/projects", server.base))
        .header("content-type", "application/json; charset=utf-8")
        .body("")
        .send()
        .await
        .unwrap();
    assert_eq!(empty.status(), 400);
    let null = server.post("/projects", Value::Null).await;
    assert_eq!(null.status(), 500);
    assert_eq!(
        null.json::<Value>().await.unwrap()["error"],
        "Cannot destructure property 'name' of '(intermediate value)' as it is null."
    );
    let huge = client
        .post(format!("{}/api/projects", server.base))
        .header("content-type", "application/json")
        .body("x".repeat(1_000_001))
        .send()
        .await
        .unwrap();
    assert_eq!(huge.status(), 413);
    assert_eq!(
        huge.json::<Value>().await.unwrap(),
        json!({"error":"request too large"})
    );
    let exact = client
        .post(format!("{}/api/projects", server.base))
        .header("content-type", "application/json")
        .body(format!("{}{{}}", " ".repeat(999_998)))
        .send()
        .await
        .unwrap();
    assert_eq!(exact.status(), 400);
    let malformed = client
        .post(format!("{}/api/projects", server.base))
        .header("content-type", "application/json")
        .body("{")
        .send()
        .await
        .unwrap();
    assert_eq!(malformed.status(), 500);
    assert!(malformed.json::<Value>().await.unwrap()["error"].is_string());
    assert_eq!(
        server.post("/projects/nope/chat", json!({})).await.status(),
        404
    );
    server.state.projects.create("rover").await.unwrap();
    assert_eq!(
        server
            .post("/projects/rover/chat", json!({"message":" "}))
            .await
            .status(),
        400
    );
    assert_eq!(
        server
            .post("/projects/rover/open", json!({}))
            .await
            .status(),
        400
    );
    assert_eq!(
        server
            .post("/projects/rover/open", json!({"path":"README.md"}))
            .await
            .status(),
        400
    );
    assert_eq!(
        server
            .post("/projects/rover/chat", json!({"message":"\u{feff}"}))
            .await
            .status(),
        400
    );
    let dist = server.temp.path.join("app/dist");
    std::fs::create_dir_all(&dist).unwrap();
    for (file, mime) in [
        ("index.html", "text/html; charset=utf-8"),
        ("x.js", "text/javascript"),
        ("x.css", "text/css"),
        ("x.svg", "image/svg+xml"),
        ("x.png", "image/png"),
        ("x.woff2", "font/woff2"),
        ("x.bin", "application/octet-stream"),
    ] {
        std::fs::write(dist.join(file), "static data").unwrap();
        let path = if file == "index.html" {
            "/".into()
        } else {
            format!("/{file}")
        };
        let response = client
            .get(format!("{}{path}", server.base))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], mime);
        assert_eq!(response.text().await.unwrap(), "static data");
    }
    assert_eq!(
        client
            .get(format!("{}/%2e%2e/server/Cargo.toml", server.base))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}
