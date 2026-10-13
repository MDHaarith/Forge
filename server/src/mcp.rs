use crate::{
    agent::{execute_tool, AgentEvent},
    http::{error, json_response, AppState, HttpError},
    tools::{available_tools, ToolContext},
};
use axum::{body::Body, http::Response};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

static CALL_IDS: AtomicU64 = AtomicU64::new(0);

/// Minimal Streamable HTTP: plain JSON also works with Claude Code's SSE Accept header.
pub(crate) async fn handle(
    state: &AppState,
    project: &str,
    request: Value,
) -> Result<Response<Body>, HttpError> {
    let Some(id) = request.get("id") else {
        return Ok(Response::builder().status(202).body(Body::empty()).unwrap());
    };
    let method = request["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => {
            json!({"protocolVersion":request["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"forge","version":env!("CARGO_PKG_VERSION")}})
        }
        "ping" => json!({}),
        "tools/list" => {
            let settings = state.settings.clone();
            let tools = tokio::task::spawn_blocking(move || available_tools(&settings))
                .await
                .map_err(|e| error(500, e.to_string()))?;
            json!({"tools":tools.iter().map(|t| json!({"name":t.name,"description":t.description,"inputSchema":t.input_schema})).collect::<Vec<_>>()})
        }
        "tools/call" => {
            let sender = if state.projects.exists(project).await {
                state.projects.live_sender(project)
            } else {
                None
            };
            let Some(sender) = sender else {
                return Ok(rpc_error(
                    id,
                    -32000,
                    "no Claude turn is running for this project",
                ));
            };
            let name = request["params"]["name"].as_str().unwrap_or("");
            let input = request["params"]
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let settings = state.settings.clone();
            let tools = tokio::task::spawn_blocking(move || available_tools(&settings))
                .await
                .map_err(|e| error(500, e.to_string()))?;
            let call_id = format!("toolu_mcp_{}", CALL_IDS.fetch_add(1, Ordering::Relaxed) + 1);
            let _ = sender.send(AgentEvent::ToolCall {
                id: call_id.clone(),
                name: name.into(),
                input: input.clone(),
            });
            let context = ToolContext::from_settings(
                state.projects.dir(project).map_err(|e| error(500, e))?,
                &state.settings,
            );
            let (ok, outcome) =
                execute_tool(&tools, name, &input, context).await;
            let _ = sender.send(AgentEvent::ToolResult {
                id: call_id,
                name: name.into(),
                ok,
                text: outcome.text.clone(),
                files: outcome.files,
            });
            let mut result = json!({"content":[{"type":"text","text":outcome.text}]});
            if !ok {
                result["isError"] = json!(true);
            }
            result
        }
        _ => return Ok(rpc_error(id, -32601, "Method not found")),
    };
    Ok(json_response(
        200,
        json!({"jsonrpc":"2.0","id":id,"result":result}),
    ))
}

fn rpc_error(id: &Value, code: i32, message: &str) -> Response<Body> {
    json_response(
        200,
        json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
    )
}
