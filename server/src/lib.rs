pub mod agent;
pub mod claude_code;
pub mod env;
pub mod http;
pub mod jsfmt;
pub mod mcp;
pub mod model;
pub mod open;
pub mod platform;
pub mod projects;
pub mod providers;
pub mod tools;

use model::Model;
use providers::{provider_label, Providers};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

/// Configuration is captured once; tests construct it without changing process environment.
#[derive(Clone)]
pub struct Settings {
    pub home: PathBuf,
    pub app_dir: PathBuf,
    pub allow_commands: bool,
    /// Resolved at startup so request handlers never probe PATH and project cwd cannot change it.
    pub claude_program: Option<PathBuf>,
    pub providers: Arc<Providers>,
    pub startup_provider: Option<String>,
    pub startup_model: Option<String>,
    pub offline: bool,
    pub blender_program: Option<PathBuf>,
    pub node_program: Option<PathBuf>,
    pub node_env: Vec<(String, String)>,
    pub openems_python: Vec<String>,
}

impl Settings {
    pub fn new(home: PathBuf, app_dir: PathBuf) -> Self {
        Self {
            home,
            app_dir: tools::paths::lexical_path(&app_dir),
            allow_commands: false,
            claude_program: None,
            providers: Arc::new(Providers::new(HashMap::new())),
            startup_provider: None,
            startup_model: None,
            offline: false,
            blender_program: None,
            node_program: None,
            node_env: vec![
                ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
                (
                    "LANG".into(),
                    std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".into()),
                ),
            ],
            openems_python: tools::rf::openems_python(),
        }
    }

    pub fn from_env(app_dir: PathBuf) -> Self {
        let vars: HashMap<String, String> = std::env::vars().collect();
        let mut settings = Self::new(
            vars.get("FORGE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| env::home_dir().join("Forge")),
            app_dir,
        );
        settings.allow_commands = vars.get("FORGE_ALLOW_COMMANDS").is_some_and(|v| v == "1");
        settings.offline = vars.get("FORGE_OFFLINE").is_some_and(|v| v == "1");
        settings.startup_provider = vars.get("FORGE_PROVIDER").cloned();
        settings.startup_model = vars.get("FORGE_MODEL").cloned();
        // FORGE_CLAUDE selects the user's CLI; its inherited environment supplies the login.
        settings.claude_program = claude_code::resolve_program(
            vars.get("FORGE_CLAUDE")
                .map(String::as_str)
                .unwrap_or("claude"),
        );
        settings.blender_program = vars
            .get("FORGE_BLENDER")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        settings.node_program = vars
            .get("FORGE_NODE")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        settings.providers = Arc::new(Providers::new(vars));
        settings
    }
}

pub use http::{router, AppState};

pub async fn serve(listener: tokio::net::TcpListener, app_dir: PathBuf) -> std::io::Result<()> {
    env::load_env_file()?;
    serve_with_settings(listener, Settings::from_env(app_dir)).await
}

pub async fn serve_with_settings(
    listener: tokio::net::TcpListener,
    settings: Settings,
) -> std::io::Result<()> {
    let state = AppState::from_settings(settings);
    let _ = state.projects.server_url.set(format!(
        "http://127.0.0.1:{}",
        listener.local_addr()?.port()
    ));
    println!(
        "Forge is running at http://127.0.0.1:{}",
        listener.local_addr()?.port()
    );
    println!(
        "Model: {} ({})",
        state.model.name(),
        provider_label(&state.model.provider())
    );
    println!("Projects: {}", state.projects.home.display());
    axum::serve(listener, router(state)).await
}
