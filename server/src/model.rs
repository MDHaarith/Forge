pub use crate::providers::{available_providers, model_choices, provider_label};
use crate::{providers::find_provider, Settings};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelRequest {
    pub system: String,
    pub tools: Vec<Value>,
    pub messages: Vec<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelReply {
    pub content: Vec<Value>,
    pub stop_reason: Option<String>,
}
pub type ModelFuture<'a> = Pin<Box<dyn Future<Output = Result<ModelReply, String>> + Send + 'a>>;

/// What the agent talks to. Claude in normal use; scripted models in tests and offline mode.
pub trait Model: Send + Sync {
    fn name(&self) -> String;
    fn provider(&self) -> Option<String> {
        None
    }
    fn respond<'a>(&'a self, request: &'a ModelRequest) -> ModelFuture<'a>;
}

static TOOL_IDS: AtomicU64 = AtomicU64::new(0);
fn tool_use(name: &str, input: Value) -> Value {
    json!({"type":"tool_use","id":format!("toolu_offline_{}",TOOL_IDS.fetch_add(1,Ordering::Relaxed)+1),"name":name,"input":input})
}
fn text(t: &str) -> Value {
    json!({"type":"text","text":t,"citations":null})
}
fn reply(reason: &str, content: Vec<Value>) -> ModelReply {
    ModelReply {
        stop_reason: Some(reason.into()),
        content,
    }
}

/// Lets Forge run end to end without an API key: it recognises a request for a 3D part,
/// builds a demo enclosure, commits it, and explains how to switch to Claude.
pub struct OfflineModel;
impl Model for OfflineModel {
    fn name(&self) -> String {
        "offline demo".into()
    }
    fn respond<'a>(&'a self, request: &'a ModelRequest) -> ModelFuture<'a> {
        Box::pin(async move {
            let last = request.messages.last().ok_or("no messages")?;
            let blocks = last["content"].as_array().cloned().unwrap_or_default();
            if let Some(result) = blocks.iter().find(|b| b["type"] == "tool_result") {
                let from = request
                    .messages
                    .iter()
                    .filter_map(|m| m["content"].as_array())
                    .flatten()
                    .find(|b| b["type"] == "tool_use" && b["id"] == result["tool_use_id"]);
                if from.is_some_and(|b| b["name"] == "part_build") && result["is_error"] != true {
                    return Ok(reply(
                        "tool_use",
                        vec![tool_use(
                            "git_commit",
                            json!({"message":"Add demo enclosure"}),
                        )],
                    ));
                }
                return Ok(reply("end_turn",vec![text("Done. The enclosure is in models/: open demo-enclosure.stl to see it in 3D, or demo-enclosure.step for CAD. This is the offline demo; sign in with the Claude Code CLI and restart Forge to work with Claude.")]));
            }
            let prompt = last["content"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    blocks
                        .iter()
                        .map(|b| {
                            if b["type"] == "text" {
                                b["text"].as_str().unwrap_or("")
                            } else {
                                ""
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                });
            let prompt = prompt.to_lowercase();
            if ["enclosure", "box", "case", "part", "3d", "model", "bracket"]
                .iter()
                .any(|s| prompt.contains(s))
            {
                let steps = json!([
                    {"id":"base","op":"enclosure","size":[60,40,20]},
                    {"op":"standoffs","target":"base","points":{"corners":[60,40,6]},"size":"M3","height":8,"base":2}
                ]);
                return Ok(reply(
                    "tool_use",
                    vec![
                        text("Building a 60 × 40 mm enclosure base with four M3 standoffs."),
                        tool_use("part_build", json!({"name":"demo-enclosure","steps":steps})),
                    ],
                ));
            }
            Ok(reply("end_turn",vec![text("Forge is running in offline demo mode, so I can only build a sample part. Ask me to \"design an enclosure\" to see the 3D pipeline, or sign in with the Claude Code CLI and restart Forge to work with Claude on code, 3D, PCB and chip designs.")]))
        })
    }
}

/// Anthropic-shaped history to OpenAI chat messages. Thinking and other Claude-only blocks are dropped.
pub fn to_chat_messages(system: &str, messages: &[Value]) -> Vec<Value> {
    let mut out = vec![json!({"role":"system","content":system})];
    for m in messages {
        if m["content"].is_string() {
            out.push(json!({"role":m["role"],"content":m["content"]}));
            continue;
        }
        let blocks = m["content"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        if m["role"] == "assistant" {
            let content = blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let calls=blocks.iter().filter(|b|b["type"]=="tool_use").map(|b|json!({"id":b["id"],"type":"function", "function":{"name":b["name"],"arguments":b["input"].to_string()}})).collect::<Vec<_>>();
            let mut message = json!({"role":"assistant","content":if content.is_empty(){Value::Null}else{json!(content)}});
            if !calls.is_empty() {
                message["tool_calls"] = json!(calls);
            }
            out.push(message);
        } else {
            for b in blocks {
                if b["type"] == "text" {
                    out.push(json!({"role":"user","content":b["text"]}));
                }
                if b["type"] == "tool_result" {
                    let content = b["content"].as_str().map(str::to_owned).unwrap_or_else(|| {
                        b["content"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .map(|c| {
                                        if c["type"] == "text" {
                                            c["text"].as_str().unwrap_or("")
                                        } else {
                                            ""
                                        }
                                    })
                                    .collect::<String>()
                            })
                            .unwrap_or_default()
                    });
                    out.push(
                        json!({"role":"tool","tool_call_id":b["tool_use_id"],"content":content}),
                    );
                }
            }
        }
    }
    out
}

/// DeepSeek, Qwen, GLM, OpenCode, OpenRouter, OpenAI: anything with an OpenAI-compatible /chat/completions.
pub struct OpenAICompatModel {
    name: String,
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}
impl OpenAICompatModel {
    pub fn new(name: String, base_url: String, api_key: String) -> Self {
        Self {
            name,
            base_url,
            api_key,
            client: reqwest::Client::new(),
        }
    }
}
impl Model for OpenAICompatModel {
    fn name(&self) -> String {
        self.name.clone()
    }
    // shortcut: not streamed, so long replies show nothing until done; switch to SSE if that hurts.
    // shortcut: DeepSeek thinking mode's reasoning_content is not echoed back; use its non-thinking model.
    fn respond<'a>(&'a self, request: &'a ModelRequest) -> ModelFuture<'a> {
        Box::pin(async move {
            let res=self.client.post(format!("{}/chat/completions",self.base_url)).bearer_auth(&self.api_key)
                .json(&json!({"model":self.name,"messages":to_chat_messages(&request.system,&request.messages),
                    "tools":request.tools.iter().map(|t|json!({"type":"function","function":{"name":t["name"],"description":t["description"],"parameters":t["input_schema"]}})).collect::<Vec<_>>()}))
                .send().await.map_err(|e|e.to_string())?;
            if !res.status().is_success() {
                let status = res.status().as_u16();
                let body = res.text().await.map_err(|e| e.to_string())?;
                return Err(format!(
                    "{}: HTTP {status} {}",
                    self.name,
                    String::from_utf16_lossy(&body.encode_utf16().take(500).collect::<Vec<_>>())
                ));
            }
            let body: Value = res.json().await.map_err(|e| e.to_string())?;
            let choice = body["choices"]
                .as_array()
                .and_then(|a| a.first())
                .ok_or_else(|| format!("{}: the reply had no choices", self.name))?;
            let mut content = Vec::new();
            if let Some(t) = choice["message"]["content"]
                .as_str()
                .filter(|s| !s.is_empty())
            {
                content.push(text(t));
            }
            if let Some(calls) = choice["message"]["tool_calls"].as_array() {
                for call in calls {
                    let arguments = call["function"]["arguments"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .unwrap_or("{}");
                    let input = serde_json::from_str(arguments)
                        .unwrap_or_else(|_| json!({"_unparsed":call["function"]["arguments"]}));
                    // Claude only accepts these id characters, and the history may later go to Claude.
                    let id = call["id"]
                        .as_str()
                        .unwrap_or("")
                        .encode_utf16()
                        .map(|unit| {
                            char::from_u32(u32::from(unit))
                                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                                .unwrap_or('_')
                        })
                        .collect::<String>();
                    let id = if id.is_empty() {
                        format!(
                            "toolu_compat_{}",
                            TOOL_IDS.fetch_add(1, Ordering::Relaxed) + 1
                        )
                    } else {
                        id
                    };
                    content.push(json!({"type":"tool_use","id":id,"name":call["function"]["name"],"input":input}));
                }
            }
            let calls = content.iter().any(|b| b["type"] == "tool_use");
            let reason = match choice["finish_reason"].as_str() {
                Some("length") => "max_tokens",
                Some("content_filter") => "refusal",
                _ if calls => "tool_use",
                _ => "end_turn",
            };
            Ok(reply(reason, content))
        })
    }
}

/// One model object shared by every project, so the in-app picker can swap what it talks to.
pub struct SwitchableModel {
    current: RwLock<(String, Arc<dyn Model>)>,
}
impl SwitchableModel {
    pub fn new(provider: impl Into<String>, current: Arc<dyn Model>) -> Self {
        Self {
            current: RwLock::new((provider.into(), current)),
        }
    }
    pub fn provider(&self) -> String {
        self.current.read().unwrap().0.clone()
    }
    pub fn set(&self, provider: impl Into<String>, model: Arc<dyn Model>) {
        *self.current.write().unwrap() = (provider.into(), model);
    }
}
impl Model for SwitchableModel {
    fn provider(&self) -> Option<String> {
        Some(self.provider())
    }
    fn name(&self) -> String {
        self.current.read().unwrap().1.name()
    }
    fn respond<'a>(&'a self, request: &'a ModelRequest) -> ModelFuture<'a> {
        let model = self.current.read().unwrap().1.clone();
        Box::pin(async move { model.respond(request).await })
    }
}

/// Callers check the id against available_providers() first.
pub fn make_model(id: &str, model: Option<&str>, settings: &Settings) -> Arc<dyn Model> {
    if id == "claude" {
        return Arc::new(crate::claude_code::ClaudeCodeModel(
            model
                .filter(|s| !s.is_empty())
                .or(settings.startup_model.as_deref().filter(|s| !s.is_empty()))
                .unwrap_or(crate::providers::CLAUDE_MODELS[0])
                .into(),
        ));
    }
    match find_provider(id) {
        Some(p) => Arc::new(OpenAICompatModel::new(
            model
                .filter(|s| !s.is_empty())
                .unwrap_or(p.default_model)
                .into(),
            settings.providers.base_url(p),
            settings.providers.api_key(p).into(),
        )),
        None => Arc::new(OfflineModel), // "offline"
    }
}

/// FORGE_PROVIDER when it is configured, else Claude when its CLI is installed, else the offline demo.
pub fn default_model(settings: &Settings) -> Arc<SwitchableModel> {
    let available = available_providers(settings);
    let id = if settings.offline {
        "offline"
    } else {
        settings
            .startup_provider
            .as_deref()
            .filter(|id| available.iter().any(|v| v == id))
            .unwrap_or(if available.iter().any(|id| id == "claude") {
                "claude"
            } else {
                "offline"
            })
    };
    let model = if settings.startup_provider.as_deref().unwrap_or("claude") == "claude" {
        None
    } else {
        settings.startup_model.as_deref()
    };
    Arc::new(SwitchableModel::new(id, make_model(id, model, settings)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converts_text_results_and_drops_thinking() {
        assert_eq!(
            to_chat_messages(
                "sys",
                &[
                    json!({"role":"assistant","content":[{"type":"thinking","thinking":"x"}]}),
                    json!({"role":"user","content":[{"type":"text","text":"hi"},{"type":"tool_result","tool_use_id":"t","content":[{"type":"text","text":"a"},{"type":"image"},{"type":"text","text":"b"}]}]})
                ]
            ),
            vec![
                json!({"role":"system","content":"sys"}),
                json!({"role":"assistant","content":null}),
                json!({"role":"user","content":"hi"}),
                json!({"role":"tool","tool_call_id":"t","content":"ab"})
            ]
        );
    }
    #[test]
    fn defaults_and_switches_with_explicit_settings() {
        let mut settings = Settings::new("/tmp/home".into(), "/tmp/app".into());
        let model = default_model(&settings);
        assert_eq!(model.provider(), "offline");
        assert_eq!(model.name(), "offline demo");
        settings.providers = Arc::new(crate::providers::Providers::new(
            std::collections::HashMap::from([("OPENAI_API_KEY".into(), "key".into())]),
        ));
        settings.startup_provider = Some("openai".into());
        settings.startup_model = Some("custom".into());
        let selected = default_model(&settings);
        assert_eq!(selected.provider(), "openai");
        assert_eq!(selected.name(), "custom");
        settings.offline = true;
        assert_eq!(default_model(&settings).provider(), "offline");
        selected.set("offline", Arc::new(OfflineModel));
        assert_eq!(selected.name(), "offline demo");
    }
}
