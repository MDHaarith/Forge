#![cfg(unix)]
mod common;
#[path = "common/claude.rs"]
mod support;

use forge_server::tools::available_tools;
use serde_json::{json, Value};
use support::{events, FakeServer, SUCCESS, WAIT};

#[tokio::test]
async fn handshake_notifications_discovery_ping_and_registry_order() {
    let server = FakeServer::start(SUCCESS).await;
    let response = reqwest::Client::new().post(format!("{}/mcp/test", server.base))
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json; charset=utf-8")
        .body(json!({"jsonrpc":"2.0","id":"initialize-1","method":"initialize","params":{"protocolVersion":"2025-11-25"}}).to_string())
        .send().await.unwrap();
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"jsonrpc":"2.0","id":"initialize-1","result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"forge","version":env!("CARGO_PKG_VERSION")}}})
    );
    let notification = server
        .post(
            "/mcp/test",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
    assert_eq!(notification.status(), 202);
    assert_eq!(notification.text().await.unwrap(), "");
    for (id, method) in [
        (json!(9), "server/discover"),
        (json!("unknown-id"), "unknown-method"),
    ] {
        let response: Value = server
            .post(
                "/mcp/test",
                json!({"jsonrpc":"2.0","id":id,"method":method}),
            )
            .await
            .json()
            .await
            .unwrap();
        assert_eq!(
            response,
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}})
        );
    }
    let ping: Value = server
        .post(
            "/mcp/test",
            json!({"jsonrpc":"2.0","id":null,"method":"ping"}),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(ping, json!({"jsonrpc":"2.0","id":null,"result":{}}));
    let list: Value = server
        .post(
            "/mcp/test",
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        )
        .await
        .json()
        .await
        .unwrap();
    let settings = server.settings.clone();
    let expected = tokio::task::spawn_blocking(move || available_tools(&settings))
        .await
        .unwrap();
    assert_eq!(
        list["result"]["tools"],
        json!(expected
            .iter()
            .map(
                |t| json!({"name":t.name,"description":t.description,"inputSchema":t.input_schema})
            )
            .collect::<Vec<_>>())
    );
    assert!(list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "write_file" && t["inputSchema"]["type"] == "object"));
    assert!(server.temp.path.exists());
}

#[tokio::test]
async fn tools_require_a_live_claude_turn_in_the_named_project() {
    let server = FakeServer::start(WAIT).await;
    for name in ["test", "missing", "invalid!", "bad%2Fname"] {
        let response: Value = server.post(&format!("/mcp/{name}"), json!({"id":"call-1","method":"tools/call","params":{"name":"write_file","arguments":{"path":"a.txt","contents":"hi"}}})).await.json().await.unwrap();
        assert_eq!(
            response,
            json!({"jsonrpc":"2.0","id":"call-1","error":{"code":-32000,"message":"no Claude turn is running for this project"}})
        );
    }
    let chat = server.chat("tools").await;
    server.wait_started().await;
    std::fs::create_dir(server.settings.home.join("other")).unwrap();
    let response: Value = server.post("/mcp/other", json!({"id":2,"method":"tools/call","params":{"name":"write_file","arguments":{"path":"a.txt","contents":"hi"}}})).await.json().await.unwrap();
    assert_eq!(response["error"]["code"], -32000);
    assert!(!server.settings.home.join("other/a.txt").exists());
    std::fs::write(server.dir().join("go"), "").unwrap();
    events(chat).await;
    let after: Value = server
        .post(
            "/mcp/test",
            json!({"id":3,"method":"tools/call","params":{"name":"write_file"}}),
        )
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(after["error"]["code"], -32000);
}

#[tokio::test]
async fn tool_calls_stream_once_with_files_and_match_saved_history() {
    let server = FakeServer::start(WAIT).await;
    let chat = server.chat("write a file").await;
    server.wait_started().await;
    let input = json!({"path":"a.txt","contents":"hi"});
    let response: Value = server.post("/mcp/test", json!({"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"write_file","arguments":input,"_meta":{"unused":true}}})).await.json().await.unwrap();
    assert_eq!(response["id"], 42);
    assert_eq!(response["jsonrpc"], "2.0");
    assert!(response["result"].get("isError").is_none());
    assert_eq!(response["result"]["content"][0]["type"], "text");
    assert_eq!(
        std::fs::read_to_string(server.dir().join("a.txt")).unwrap(),
        "hi"
    );
    std::fs::write(server.dir().join("go"), "").unwrap();
    let streamed = events(chat).await;
    assert_eq!(
        streamed
            .iter()
            .map(|e| e["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["user", "tool_call", "tool_result", "done"]
    );
    assert_eq!(streamed[1]["name"], "write_file");
    assert_eq!(streamed[1]["input"], input);
    assert_eq!(streamed[2]["id"], streamed[1]["id"]);
    assert_eq!(streamed[2]["name"], "write_file");
    assert_eq!(streamed[2]["ok"], true);
    assert_eq!(streamed[2]["files"], json!(["a.txt"]));
    assert_eq!(
        streamed[2]["text"],
        response["result"]["content"][0]["text"]
    );
    assert_eq!(server.history().await["events"], json!(streamed));
    let saved: Value = serde_json::from_slice(
        &std::fs::read(server.settings.home.join(".state/test.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved["events"], json!(streamed));
    assert_eq!(saved["messages"], json!([]));
}

#[tokio::test]
async fn in_flight_tool_result_precedes_done_after_cli_exit() {
    use std::time::Duration;
    let server = FakeServer::start(&format!("{WAIT}touch exited\n")).await;
    let pipe = server.dir().join("slow.txt");
    assert!(std::process::Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .unwrap()
        .success());
    let chat = server.chat("slow tool").await;
    server.wait_started().await;
    let base = server.base.clone();
    let call = tokio::spawn(async move {
        reqwest::Client::new().post(format!("{base}/mcp/test"))
            .json(&json!({"id":1,"method":"tools/call","params":{"name":"write_file","arguments":{"path":"slow.txt","contents":"hi"}}}))
            .send().await.unwrap().json::<Value>().await.unwrap()
    });
    tokio::time::timeout(Duration::from_secs(10), async {
        while server.history().await["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["type"] != "tool_call")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    std::fs::write(server.dir().join("go"), "").unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !server.dir().join("exited").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(server.history().await["running"], true);
    // Opening the FIFO releases write_file only after the CLI has gone away.
    let data = tokio::task::spawn_blocking(move || std::fs::read(pipe).unwrap())
        .await
        .unwrap();
    assert_eq!(data, b"hi");
    assert!(call.await.unwrap()["result"].get("isError").is_none());
    let streamed = events(chat).await;
    assert_eq!(
        streamed
            .iter()
            .map(|e| e["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["user", "tool_call", "tool_result", "done"]
    );
    assert_eq!(streamed[2]["files"], json!(["slow.txt"]));
    assert_eq!(server.history().await["events"], json!(streamed));
}

#[tokio::test]
async fn tool_errors_keep_validation_unavailable_and_path_escape_wording() {
    let server = FakeServer::start(WAIT).await;
    let chat = server.chat("errors").await;
    server.wait_started().await;
    for (name, arguments, expected) in [
        (
            "write_file",
            Some(json!({"path":"../x","contents":"hi"})),
            "path \"../x\" is outside the project",
        ),
        (
            "write_file",
            Some(json!({"path":".git/config","contents":"hi"})),
            "the .git folder is managed by the git tools",
        ),
        (
            "unknown",
            Some(json!({})),
            "Unknown or unavailable tool \"unknown\".",
        ),
        (
            "run_command",
            Some(json!({"command":"pwd"})),
            "Unknown or unavailable tool \"run_command\".",
        ),
        (
            "write_file",
            None,
            r#"{"INVALID_INPUT":["input.path is required","input.contents is required"],"received":{}}"#,
        ),
    ] {
        let mut params = json!({"name":name});
        if let Some(arguments) = arguments {
            params["arguments"] = arguments;
        }
        let response: Value = server
            .post(
                "/mcp/test",
                json!({"id":"bad-call","method":"tools/call","params":params}),
            )
            .await
            .json()
            .await
            .unwrap();
        assert_eq!(
            response["result"],
            json!({"content":[{"type":"text","text":expected}],"isError":true})
        );
    }
    assert!(!server.settings.home.join("x").exists());
    assert!(!server.dir().join(".git").exists());
    std::fs::write(server.dir().join("go"), "").unwrap();
    let streamed = events(chat).await;
    assert_eq!(
        streamed.iter().filter(|e| e["type"] == "tool_call").count(),
        5
    );
    assert_eq!(
        streamed
            .iter()
            .filter(|e| e["type"] == "tool_result" && e["ok"] == false)
            .count(),
        5
    );
    assert_eq!(streamed.last().unwrap()["type"], "done");
    assert_eq!(server.history().await["events"], json!(streamed));
}

#[tokio::test]
async fn mcp_shares_host_json_and_body_limits_and_rejects_get_delete() {
    let server = FakeServer::start(SUCCESS).await;
    let url = format!("{}/mcp/test", server.base);
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .post(&url)
            .header("host", "evil.example")
            .json(&json!({"id":1,"method":"ping"}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    for content_type in [
        "text/plain",
        "application/jsonp",
        "application/x-www-form-urlencoded",
    ] {
        assert_eq!(
            client
                .post(&url)
                .header("content-type", content_type)
                .body("{}")
                .send()
                .await
                .unwrap()
                .status(),
            415
        );
    }
    assert_eq!(
        client
            .post(&url)
            .header("content-type", "application/json")
            .body(" ".repeat(1_000_001))
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    assert_eq!(client.get(&url).send().await.unwrap().status(), 405);
    assert_eq!(client.delete(&url).send().await.unwrap().status(), 405);
}
