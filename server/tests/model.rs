use axum::{
    body::{to_bytes, Body},
    http::Request,
    response::Response,
    Router,
};
use forge_server::model::{Model, ModelRequest, OpenAICompatModel};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// A local HTTP stub that records the request body and answers with `reply`.
struct Stub {
    url: String,
    sent: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Stub {
    async fn start(reply: Value, status: u16) -> Self {
        let sent = Arc::new(Mutex::new(vec![]));
        let recorded = sent.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let app = Router::new().fallback(move |request: Request<Body>| {
            let sent = recorded.clone();
            let reply = reply.clone();
            async move {
                assert_eq!(request.uri().path(), "/v1/chat/completions");
                assert_eq!(request.headers()["authorization"], "Bearer key");
                assert_eq!(request.headers()["content-type"], "application/json");
                let body = to_bytes(request.into_body(), 1_000_000).await.unwrap();
                sent.lock()
                    .unwrap()
                    .push(serde_json::from_slice(&body).unwrap());
                Response::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(Body::from(reply.to_string()))
                    .unwrap()
            }
        });
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, sent, task }
    }
    fn model(&self, name: &str) -> OpenAICompatModel {
        OpenAICompatModel::new(name.into(), self.url.clone(), "key".into())
    }
}
impl Drop for Stub {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn tools() -> Vec<Value> {
    vec![
        json!({"name":"cad_model","description":"Build a part","input_schema":{"type":"object","properties":{}}}),
    ]
}
fn request() -> ModelRequest {
    ModelRequest {
        system: "".into(),
        tools: tools(),
        messages: vec![json!({"role":"user","content":"x"})],
    }
}

#[tokio::test]
async fn history_and_tools_are_sent_in_openai_chat_format() {
    let stub = Stub::start(
        json!({"choices":[{"finish_reason":"stop","message":{"content":"Done."}}]}),
        200,
    )
    .await;
    let reply=stub.model("m1").respond(&ModelRequest{system:"sys".into(),tools:tools(),messages:vec![
        json!({"role":"user","content":"make a box"}),
        json!({"role":"assistant","content":[{"type":"thinking","thinking":"hmm","signature":"s"},{"type":"text","text":"Building."},
            {"type":"tool_use","id":"t1","name":"cad_model","input":{"name":"box"}}]}),
        json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]})]}).await.unwrap();
    let sent = stub.sent.lock().unwrap();
    assert_eq!(
        sent[0]["messages"],
        json!([
        {"role":"system","content":"sys"},{"role":"user","content":"make a box"},
        {"role":"assistant","content":"Building.","tool_calls":[{"id":"t1","type":"function","function":{"name":"cad_model","arguments":"{\"name\":\"box\"}"}}]},
        {"role":"tool","tool_call_id":"t1","content":"ok"}])
    );
    assert_eq!(
        sent[0]["tools"],
        json!([{"type":"function","function":{"name":"cad_model","description":"Build a part","parameters":tools()[0]["input_schema"]}}])
    );
    assert_eq!(sent[0]["model"], "m1");
    assert_eq!(reply.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(
        reply
            .content
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["text"]
    );
}

async fn mapped(choice: Value) -> forge_server::model::ModelReply {
    let stub = Stub::start(json!({"choices":[choice]}), 200).await;
    stub.model("m").respond(&request()).await.unwrap()
}
#[tokio::test]
async fn tool_calls_cut_off_and_bad_arguments_map_back() {
    let called=mapped(json!({"finish_reason":"tool_calls","message":{"content":null,"tool_calls":[{"id":"functions.cad:0","function":{"name":"cad_model","arguments":"{\"a\":1}"}}]}})).await;
    assert_eq!(called.stop_reason.as_deref(), Some("tool_use"));
    assert_eq!(
        called.content,
        vec![json!({"type":"tool_use","id":"functions_cad_0","name":"cad_model","input":{"a":1}})]
    );
    let unicode = mapped(json!({"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"fn.😀","function":{"name":"cad_model","arguments":"{}"}}]}})).await;
    assert_eq!(unicode.content[0]["id"], "fn___");
    let bad=mapped(json!({"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"c","function":{"name":"cad_model","arguments":"{oops"}}]}})).await;
    assert_eq!(bad.content[0]["input"], json!({"_unparsed":"{oops"}));
    assert_eq!(
        mapped(json!({"finish_reason":"length","message":{"content":"par"}}))
            .await
            .stop_reason
            .as_deref(),
        Some("max_tokens")
    );
    assert_eq!(
        mapped(json!({"finish_reason":"content_filter","message":{}}))
            .await
            .stop_reason
            .as_deref(),
        Some("refusal")
    );
    let empty=mapped(json!({"finish_reason":"tool_calls","message":{"tool_calls":[{"function":{"name":"cad_model"}}]}})).await;
    assert!(empty.content[0]["id"]
        .as_str()
        .unwrap()
        .starts_with("toolu_compat_"));
    assert_eq!(empty.content[0]["input"], json!({}));
}

#[tokio::test]
async fn http_errors_carry_status_and_body() {
    let stub = Stub::start(json!({"error":"bad key"}), 401).await;
    let error = stub.model("m").respond(&request()).await.unwrap_err();
    assert_eq!(error, "m: HTTP 401 {\"error\":\"bad key\"}");
}

#[tokio::test]
async fn missing_choices_and_error_body_limit() {
    let stub = Stub::start(json!({}), 200).await;
    assert_eq!(
        stub.model("m").respond(&request()).await.unwrap_err(),
        "m: the reply had no choices"
    );
    let stub = Stub::start(json!("x".repeat(600)), 503).await;
    let error = stub.model("m").respond(&request()).await.unwrap_err();
    assert_eq!(error.len(), "m: HTTP 503 ".len() + 500);
}
