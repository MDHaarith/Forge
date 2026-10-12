use crate::{
    agent::{Agent, AgentEvent, Emit},
    model::Model,
    tools::{available_tools, paths::project_path, vcs::init_repo, ToolContext},
    Settings,
};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
use tokio::sync::{mpsc::UnboundedSender, OnceCell};

pub struct Session {
    pub agent: Arc<Agent>,
    pub events: Mutex<Vec<AgentEvent>>,
    pub claude_session: Mutex<Option<String>>,
    /// MCP tasks enqueue events; only the turn task writes history and the chat stream.
    pub live: Mutex<Option<UnboundedSender<AgentEvent>>>,
}

/// Projects are folders under the Forge home (default ~/Forge), each its own git repository.
/// Conversations are saved beside them in .state/ so they survive restarts without being committed.
pub struct Projects {
    sessions: Mutex<HashMap<String, Arc<OnceCell<Arc<Session>>>>>,
    pub home: PathBuf,
    pub(crate) model: Arc<dyn Model>,
    pub(crate) settings: Settings,
    /// The listener's actual loopback URL, including its dynamically allocated port.
    pub server_url: OnceLock<String>,
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || (i > 0 && (b == b'-' || b == b'_'))
        })
}

impl Projects {
    pub fn new(settings: Settings, model: Arc<dyn Model>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            home: settings.home.clone(),
            model,
            settings,
            server_url: OnceLock::new(),
        }
    }
    pub async fn list(&self) -> Result<Vec<String>, String> {
        tokio::fs::create_dir_all(&self.home)
            .await
            .map_err(|e| e.to_string())?;
        let mut entries = tokio::fs::read_dir(&self.home)
            .await
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Only folders Forge made (git repo or saved chat), not every folder in a shared home.
            if valid_name(&name)
                && entry.file_type().await.map_err(|e| e.to_string())?.is_dir()
                && (entry.path().join(".git").exists() || self.state_path(&name).exists())
            {
                out.push(name);
            }
        }
        out.sort();
        Ok(out)
    }
    pub fn dir(&self, name: &str) -> Result<PathBuf, String> {
        if !valid_name(name) {
            return Err("project names use lowercase letters, digits, - and _".into());
        }
        Ok(self.home.join(name))
    }
    pub async fn exists(&self, name: &str) -> bool {
        match self.dir(name) {
            Ok(dir) => tokio::fs::metadata(dir).await.is_ok_and(|m| m.is_dir()),
            Err(_) => false,
        }
    }
    pub async fn create(&self, name: &str) -> Result<(), String> {
        let dir = self.dir(name)?;
        if self.exists(name).await {
            return Err(format!("project {name} already exists"));
        }
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| e.to_string())?;
        let readme = project_path(&dir, "README.md").map_err(|e| e.to_string())?;
        tokio::fs::write(readme,format!("# {name}\n\nA Forge project. Designs live here as text: code, models/, pcb/, chip/, fab/.\n")).await.map_err(|e|e.to_string())?;
        init_repo(&dir).await.map_err(|e| e.to_string())
    }
    /// True while any project's agent is working.
    pub fn running(&self) -> bool {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .any(|cell| cell.get().is_some_and(|s| s.agent.running()))
    }
    fn state_path(&self, name: &str) -> PathBuf {
        self.home.join(".state").join(format!("{name}.json"))
    }
    pub async fn session(&self, name: &str) -> Result<Arc<Session>, String> {
        let dir = self.dir(name)?;
        let cell = self
            .sessions
            .lock()
            .unwrap()
            .entry(name.into())
            .or_default()
            .clone();
        Ok(cell
            .get_or_init(|| async {
                let settings = self.settings.clone();
                let agent = Arc::new(Agent::new(
                    self.model.clone(),
                    name,
                    ToolContext::from_settings(dir, &self.settings),
                    Arc::new(move || available_tools(&settings)),
                ));
                let session = Arc::new(Session {
                    agent,
                    events: Mutex::new(vec![]),
                    claude_session: Mutex::new(None),
                    live: Mutex::new(None),
                });
                if let Ok(data) = tokio::fs::read(self.state_path(name)).await {
                    if let Ok(saved) = serde_json::from_slice::<Value>(&data) {
                        if let Some(messages) = saved["messages"].as_array() {
                            session
                                .agent
                                .messages
                                .lock()
                                .unwrap()
                                .extend(messages.clone());
                        }
                        if let Ok(events) =
                            serde_json::from_value::<Vec<AgentEvent>>(saved["events"].clone())
                        {
                            session.events.lock().unwrap().extend(events);
                        }
                        *session.claude_session.lock().unwrap() =
                            saved["claude_session"].as_str().map(str::to_owned);
                    }
                }
                session
            })
            .await
            .clone())
    }
    pub async fn begin_chat(&self, name: &str) -> Result<Arc<Session>, String> {
        let session = self.session(name).await?;
        session.agent.try_start()?;
        Ok(session)
    }
    pub(crate) fn live_sender(&self, name: &str) -> Option<UnboundedSender<AgentEvent>> {
        let cell = self.sessions.lock().unwrap().get(name).cloned()?;
        let sender = cell.get()?.live.lock().unwrap().clone();
        sender
    }
    pub async fn chat(&self, name: &str, text: &str, emit: &Emit<'_>) -> Result<(), String> {
        let session = self.begin_chat(name).await?;
        self.chat_started(name, session, text, emit).await
    }
    pub async fn chat_started(
        &self,
        name: &str,
        session: Arc<Session>,
        text: &str,
        emit: &Emit<'_>,
    ) -> Result<(), String> {
        let record = |event: AgentEvent| {
            session.events.lock().unwrap().push(event.clone());
            emit(event);
        };
        if self.model.provider().as_deref() == Some("claude") {
            let _busy = session.agent.turn_guard();
            // shortcut: switching providers does not carry history between Claude Code and Forge; add a summary hand-off if users switch often.
            record(AgentEvent::User { text: text.into() });
            let result = std::panic::AssertUnwindSafe(crate::claude_code::send(
                self, name, &session, text, &record,
            ))
            .catch_unwind()
            .await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(message)) => record(AgentEvent::Error { message }),
                Err(_) => record(AgentEvent::Error {
                    message: "Claude Code turn panicked".into(),
                }),
            }
            record(AgentEvent::Done);
            return self.save(name, &session).await;
        }
        session.agent.send_started(text, &record).await;
        self.save(name, &session).await
    }
    pub(crate) async fn save(&self, name: &str, session: &Session) -> Result<(), String> {
        let mut saved = json!({"messages":session.agent.messages.lock().unwrap().clone(),"events":session.events.lock().unwrap().clone()});
        if let Some(id) = session.claude_session.lock().unwrap().clone() {
            saved["claude_session"] = json!(id);
        }
        let state = self.state_path(name);
        tokio::fs::create_dir_all(state.parent().unwrap())
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::write(state, saved.to_string())
            .await
            .map_err(|e| e.to_string())
    }
}
