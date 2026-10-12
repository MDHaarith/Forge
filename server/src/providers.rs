use crate::Settings;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::OnceCell;

/// OpenAI-compatible providers. Subscription "coding plans" (GLM, Qwen, OpenCode) are an API key on a
/// plan-specific endpoint: point FORGE_<ID>_BASE_URL at it. Keys live in ~/.config/forge/env.
pub struct Provider {
    pub id: &'static str,
    pub label: &'static str,
    pub base_url: &'static str,
    pub key_env: &'static str,
    /// Used until the provider's model list loads; any model it lists can be picked instead.
    pub default_model: &'static str,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "openai",
        label: "OpenAI",
        base_url: "https://api.openai.com/v1",
        key_env: "OPENAI_API_KEY",
        default_model: "gpt-5",
    },
    Provider {
        id: "openrouter",
        label: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        key_env: "OPENROUTER_API_KEY",
        default_model: "openai/gpt-5",
    },
    Provider {
        id: "deepseek",
        label: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        key_env: "DEEPSEEK_API_KEY",
        default_model: "deepseek-chat",
    },
    Provider {
        id: "qwen",
        label: "Qwen",
        base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        key_env: "DASHSCOPE_API_KEY",
        default_model: "qwen3-coder-plus",
    },
    Provider {
        id: "glm",
        label: "GLM (Z.ai)",
        base_url: "https://api.z.ai/api/coding/paas/v4",
        key_env: "ZAI_API_KEY",
        default_model: "glm-4.6",
    },
    Provider {
        id: "opencode",
        label: "OpenCode Zen",
        base_url: "https://opencode.ai/zen/v1",
        key_env: "OPENCODE_API_KEY",
        default_model: "qwen3-coder",
    },
];

pub fn find_provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

type ModelList = Arc<OnceCell<Option<Vec<String>>>>;

pub struct Providers {
    vars: HashMap<String, String>,
    lists: Mutex<HashMap<String, ModelList>>,
    client: reqwest::Client,
}

impl Providers {
    pub fn new(vars: HashMap<String, String>) -> Self {
        Self {
            vars,
            lists: Mutex::new(HashMap::new()),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        }
    }
    pub fn base_url(&self, p: &Provider) -> String {
        self.vars
            .get(&format!("FORGE_{}_BASE_URL", p.id.to_uppercase()))
            .map(String::as_str)
            .unwrap_or(p.base_url)
            .trim_end_matches('/')
            .into()
    }
    pub fn api_key(&self, p: &Provider) -> &str {
        self.vars.get(p.key_env).map(String::as_str).unwrap_or("")
    }

    /// The provider's model ids from GET /models, cached; failures are not cached, so a later call retries.
    pub async fn list_models(&self, p: &Provider) -> Vec<String> {
        let list = self
            .lists
            .lock()
            .unwrap()
            .entry(p.id.into())
            .or_default()
            .clone();
        let result = list
            .get_or_init(|| async {
                let response = self
                    .client
                    .get(format!("{}/models", self.base_url(p)))
                    .bearer_auth(self.api_key(p))
                    .send()
                    .await
                    .ok()?
                    .error_for_status()
                    .ok()?;
                let body: serde_json::Value = response.json().await.ok()?;
                let mut ids = Vec::new();
                if let Some(data) = body.get("data") {
                    if !data.is_null() {
                        for model in data.as_array()? {
                            ids.push(model["id"].as_str()?.to_owned());
                        }
                    }
                }
                ids.sort();
                Some(ids)
            })
            .await;
        if result.is_none() {
            let mut cache = self.lists.lock().unwrap();
            if cache
                .get(p.id)
                .is_some_and(|value| Arc::ptr_eq(value, &list))
            {
                cache.remove(p.id);
            }
        }
        result
            .as_ref()
            .filter(|ids| !ids.is_empty())
            .cloned()
            .unwrap_or_else(|| vec![p.default_model.into()])
    }
}

/// Provider ids with credentials configured, in picker order; the offline demo is always there.
pub fn available_providers(settings: &Settings) -> Vec<String> {
    let mut ids = Vec::new();
    if settings
        .claude_program
        .as_ref()
        .is_some_and(|p| p.is_file())
    {
        ids.push("claude".into());
    }
    ids.extend(
        PROVIDERS
            .iter()
            .filter(|p| !settings.providers.api_key(p).is_empty())
            .map(|p| p.id.to_owned()),
    );
    ids.push("offline".into());
    ids
}

pub fn provider_label(id: &str) -> &str {
    if id == "claude" {
        "Claude (Claude Code)"
    } else if id == "offline" {
        "Offline demo"
    } else {
        find_provider(id).map(|p| p.label).unwrap_or(id)
    }
}

pub async fn model_choices(id: &str, settings: &Settings) -> Vec<String> {
    if id == "claude" {
        return CLAUDE_MODELS.iter().map(|id| (*id).into()).collect();
    }
    if id == "offline" {
        return vec!["offline demo".into()];
    }
    match find_provider(id) {
        Some(p) => settings.providers.list_models(p).await,
        None => vec![],
    }
}

pub const CLAUDE_MODELS: &[&str] = &["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        response::Response,
        Router,
    };
    use serde_json::json;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    #[test]
    fn provider_table_and_explicit_config() {
        assert_eq!(
            PROVIDERS.iter().map(|p| p.id).collect::<Vec<_>>(),
            [
                "openai",
                "openrouter",
                "deepseek",
                "qwen",
                "glm",
                "opencode"
            ]
        );
        let providers = Providers::new(HashMap::from([
            ("OPENAI_API_KEY".into(), "key".into()),
            (
                "FORGE_OPENAI_BASE_URL".into(),
                "http://localhost/v1///".into(),
            ),
        ]));
        let p = find_provider("openai").unwrap();
        assert_eq!(providers.base_url(p), "http://localhost/v1");
        assert_eq!(providers.api_key(p), "key");
        let mut settings = Settings::new(PathBuf::from("/tmp/home"), PathBuf::from("/tmp/app"));
        settings.providers = Arc::new(providers);
        assert_eq!(available_providers(&settings), ["openai", "offline"]);
        assert_eq!(provider_label("glm"), "GLM (Z.ai)");
        assert_eq!(provider_label("offline"), "Offline demo");
        assert_eq!(provider_label("custom"), "custom");
        assert!(find_provider("missing").is_none());
    }
    #[tokio::test]
    async fn lists_cache_success_and_retry_failure() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(move |req: Request<Body>| {
            let count = count.clone();
            async move {
                assert_eq!(req.uri().path(), "/models");
                assert_eq!(req.headers()["authorization"], "Bearer key");
                let n = count.fetch_add(1, Ordering::SeqCst);
                Response::builder()
                    .status(if n == 0 {
                        StatusCode::UNAUTHORIZED
                    } else {
                        StatusCode::OK
                    })
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"data":[{"id":"z"},{"id":"a"}]}).to_string(),
                    ))
                    .unwrap()
            }
        });
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let providers = Providers::new(HashMap::from([
            ("OPENAI_API_KEY".into(), "key".into()),
            ("FORGE_OPENAI_BASE_URL".into(), url),
        ]));
        let p = find_provider("openai").unwrap();
        assert_eq!(providers.list_models(p).await, ["gpt-5"]);
        let (a, b) = tokio::join!(providers.list_models(p), providers.list_models(p));
        assert_eq!(a, ["a", "z"]);
        assert_eq!(a, b);
        assert_eq!(providers.list_models(p).await, a);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        server.abort();
    }
}
