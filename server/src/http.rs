use crate::jsfmt::trim;
use crate::{
    model::{default_model, make_model, Model, SwitchableModel},
    open::{open_in_app, opener_types},
    projects::Projects,
    providers::{available_providers, model_choices},
    tools::{
        describe_tools,
        paths::{lexical_path, project_path},
        workspace::list_files,
    },
    Settings,
};
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{Request, Response},
    Router,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};
use tokio_stream::wrappers::UnboundedReceiverStream;

#[derive(Clone)]
pub struct AppState {
    pub projects: Arc<Projects>,
    pub model: Arc<SwitchableModel>,
    pub settings: Arc<Settings>,
}
impl AppState {
    pub fn new(projects: Arc<Projects>, model: Arc<SwitchableModel>, settings: Settings) -> Self {
        Self {
            projects,
            model,
            settings: Arc::new(settings),
        }
    }
    pub fn from_settings(settings: Settings) -> Self {
        let model = default_model(&settings);
        let projects = Arc::new(Projects::new(settings.clone(), model.clone()));
        Self::new(projects, model, settings)
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .fallback(handle)
        .layer(DefaultBodyLimit::disable())
        .with_state(state)
}

#[derive(Debug)]
struct HttpError {
    status: u16,
    message: String,
}
fn error(status: u16, message: impl Into<String>) -> HttpError {
    HttpError {
        status,
        message: message.into(),
    }
}
fn json_response(status: u16, body: Value) -> Response<Body> {
    bytes_response(status, "application/json", body.to_string())
}
fn bytes_response(status: u16, content_type: &str, body: impl Into<Body>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(body.into())
        .unwrap()
}

async fn handle(State(state): State<AppState>, request: Request<Body>) -> Response<Body> {
    match route(state, request).await {
        Ok(response) => response,
        Err(e) => json_response(e.status, json!({"error":e.message})),
    }
}

async fn read_json(request: Request<Body>, field: &str) -> Result<Value, HttpError> {
    // Requiring JSON forces a CORS preflight, so other websites cannot post to Forge.
    if !request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
    {
        return Err(error(415, "expected application/json"));
    }
    let mut stream = request.into_body().into_data_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| error(500, e.to_string()))?;
        if body.len() + chunk.len() > 1_000_000 {
            return Err(error(413, "request too large"));
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_slice(&body).map_err(|e| error(500, e.to_string()))?;
    if value.is_null() {
        return Err(error(
            500,
            format!(
                "Cannot destructure property '{field}' of '(intermediate value)' as it is null."
            ),
        ));
    }
    Ok(value)
}

fn decode_bytes(text: &str, strict: bool, plus: bool) -> Result<Vec<u8>, HttpError> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .and_then(|digits| std::str::from_utf8(digits).ok())
                .and_then(|digits| u8::from_str_radix(digits, 16).ok());
            if let Some(value) = hex {
                out.push(value);
                i += 3;
                continue;
            }
            if strict {
                return Err(error(500, "URI malformed"));
            }
        }
        out.push(if plus && bytes[i] == b'+' {
            b' '
        } else {
            bytes[i]
        });
        i += 1;
    }
    Ok(out)
}
fn decode_segment(segment: &str) -> Result<String, HttpError> {
    String::from_utf8(decode_bytes(segment, true, false)?).map_err(|_| error(500, "URI malformed"))
}
fn query_value(query: Option<&str>, key: &str) -> String {
    for pair in query.unwrap_or("").split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = decode_bytes(name, false, true).unwrap();
        if String::from_utf8_lossy(&name) == key {
            return String::from_utf8_lossy(&decode_bytes(value, false, true).unwrap())
                .into_owned();
        }
    }
    String::new()
}

fn value_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => crate::jsfmt::number(n.as_f64().unwrap()),
        Value::Array(a) => a.iter().map(value_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

async fn route(state: AppState, request: Request<Body>) -> Result<Response<Body>, HttpError> {
    // Only answer to localhost names, which blocks DNS-rebinding attacks from web pages.
    let raw_host = request
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let host = raw_host
        .rsplit_once(':')
        .filter(|(_, port)| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
        .map(|(host, _)| host)
        .unwrap_or(raw_host);
    if !["127.0.0.1", "localhost", "[::1]"].contains(&host) {
        return Err(error(403, "forbidden host"));
    }
    let method = request.method().as_str().to_owned();
    let url = reqwest::Url::parse("http://localhost")
        .unwrap()
        .join(&request.uri().to_string())
        .map_err(|_| error(500, "Invalid URL"))?;
    let path = url.path().to_owned();
    let query = url.query().map(str::to_owned);
    let parts = path
        .split('/')
        .filter(|p| !p.is_empty())
        .map(decode_segment)
        .collect::<Result<Vec<_>, _>>()?;
    if parts.first().map(String::as_str) != Some("api") {
        return serve_static(&state.settings.app_dir, &path).await;
    }
    if method == "GET" && parts.get(1).map(String::as_str) == Some("status") && parts.len() == 2 {
        let settings = state.settings.clone();
        let (tools, open) = tokio::task::spawn_blocking(move || {
            (describe_tools(&settings), opener_types(&settings))
        })
        .await
        .map_err(|e| error(500, e.to_string()))?;
        return Ok(json_response(
            200,
            json!({"model":state.model.name(),"offline":state.model.name()=="offline demo","home":state.projects.home,"tools":tools,"open":open}),
        ));
    }
    if method == "GET" && parts.get(1).map(String::as_str) == Some("models") && parts.len() == 2 {
        let ids = available_providers(&state.settings);
        let settings = &state.settings;
        let providers = futures_util::future::join_all(ids.iter().map(|id| async move {
            json!({"id":id,"label":crate::providers::provider_label(id),"models":model_choices(id,settings).await})
        })).await;
        return Ok(json_response(
            200,
            json!({"provider":state.model.provider(),"model":state.model.name(),"providers":providers}),
        ));
    }
    if method == "POST" && parts.get(1).map(String::as_str) == Some("model") && parts.len() == 2 {
        let body = read_json(request, "provider").await?;
        let provider = body["provider"]
            .as_str()
            .filter(|id| available_providers(&state.settings).iter().any(|v| v == id))
            .ok_or_else(|| error(400, "unknown or unconfigured provider"))?;
        if body.get("model").is_some_and(|v| !v.is_string()) {
            return Err(error(400, "model must be a string"));
        }
        if state.projects.running() {
            return Err(error(
                409,
                "Forge is still working; switch models when it is done",
            ));
        }
        state.model.set(
            provider,
            make_model(provider, body["model"].as_str().map(trim), &state.settings),
        );
        return Ok(json_response(
            200,
            json!({"provider":provider,"model":state.model.name()}),
        ));
    }
    if parts.get(1).map(String::as_str) == Some("projects") && parts.len() == 2 {
        if method == "GET" {
            return Ok(json_response(
                200,
                json!(state.projects.list().await.map_err(|e| error(500, e))?),
            ));
        }
        if method == "POST" {
            let body = read_json(request, "name").await?;
            state
                .projects
                .create(&value_string(&body["name"]))
                .await
                .map_err(|e| error(400, e))?;
            return Ok(json_response(201, json!({"name":body["name"]})));
        }
    }
    if parts.get(1).map(String::as_str) == Some("projects") && parts.len() == 4 {
        let name = &parts[2];
        if !state.projects.exists(name).await {
            return Err(error(404, "no such project"));
        }
        let dir = state.projects.dir(name).map_err(|e| error(500, e))?;
        let action = parts[3].as_str();
        if method == "GET" && action == "files" {
            return Ok(json_response(
                200,
                json!(list_files(&dir)
                    .await
                    .map_err(|e| error(500, e.to_string()))?),
            ));
        }
        if method == "GET" && action == "file" {
            let full = project_path(&dir, &query_value(query.as_deref(), "path"))
                .map_err(|e| error(400, e.to_string()))?;
            let data = tokio::fs::read(full)
                .await
                .map_err(|_| error(404, "no such file"))?;
            return Ok(bytes_response(200, "application/octet-stream", data));
        }
        if method == "GET" && action == "history" {
            let session = state
                .projects
                .session(name)
                .await
                .map_err(|e| error(500, e))?;
            return Ok(json_response(
                200,
                json!({"events":session.events.lock().unwrap().clone(),"running":session.agent.running()}),
            ));
        }
        if method == "POST" && action == "open" {
            let body = read_json(request, "path").await?;
            let rel = body["path"]
                .as_str()
                .ok_or_else(|| error(400, "path is required"))?;
            let app = open_in_app(&dir, rel, &state.settings)
                .await
                .map_err(|e| error(e.status, e.message))?;
            return Ok(json_response(200, json!({"app":app})));
        }
        if method == "POST" && action == "chat" {
            let body = read_json(request, "message").await?;
            let message = body["message"]
                .as_str()
                .map(trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| error(400, "message is required"))?
                .to_owned();
            let session = state
                .projects
                .begin_chat(name)
                .await
                .map_err(|_| error(409, "Forge is still working on this project"))?;
            // Events stream back as newline-delimited JSON while the agent works.
            let (sender, receiver) =
                tokio::sync::mpsc::unbounded_channel::<Result<Bytes, Infallible>>();
            let projects = state.projects.clone();
            let name = name.clone();
            tokio::spawn(async move {
                let emit = |event| {
                    let line = serde_json::to_string(&event).unwrap() + "\n";
                    let _ = sender.send(Ok(Bytes::from(line)));
                };
                if let Err(error) = projects.chat_started(&name, session, &message, &emit).await {
                    eprintln!("could not save project {name}: {error}");
                }
            });
            return Ok(Response::builder()
                .status(200)
                .header("content-type", "application/x-ndjson")
                .header("cache-control", "no-cache")
                .body(Body::from_stream(UnboundedReceiverStream::new(receiver)))
                .unwrap());
        }
    }
    Err(error(404, "not found"))
}

async fn serve_static(app_dir: &std::path::Path, path: &str) -> Result<Response<Body>, HttpError> {
    let dist = lexical_path(&app_dir.join("dist"));
    let rel = if path == "/" {
        "index.html"
    } else {
        path.strip_prefix('/').unwrap_or(path)
    };
    let full = lexical_path(&dist.join(rel));
    if full == dist || !full.starts_with(&dist) {
        return Err(error(404, "not found"));
    }
    let data = tokio::fs::read(&full).await.map_err(|_| {
        if path == "/" {
            error(
                503,
                "the interface is not built yet: run `npm start`, or `npm run dev` for development",
            )
        } else {
            error(404, "not found")
        }
    })?;
    let content_type = match full.extension().and_then(|v| v.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    Ok(bytes_response(200, content_type, data))
}
