pub mod agent;
pub mod env;
pub mod http;
pub mod jsfmt;
pub mod model;
pub mod open;
pub mod platform;
pub mod projects;
pub mod providers;
pub mod tools;

use providers::Providers;
use std::{collections::HashMap, path::PathBuf, sync::Arc};

/// Configuration is captured once; tests construct it without changing process environment.
#[derive(Clone)]
pub struct Settings {
    pub home: PathBuf,
    pub app_dir: PathBuf,
    pub allow_commands: bool,
    pub claude_program: Option<PathBuf>,
    pub providers: Arc<Providers>,
    pub startup_provider: Option<String>,
    pub startup_model: Option<String>,
    pub offline: bool,
    pub blender_program: Option<PathBuf>,
    pub freecad_program: Option<PathBuf>,
    pub freecad_gui_program: Option<PathBuf>,
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
            freecad_program: None,
            freecad_gui_program: None,
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
        settings.blender_program = vars
            .get("FORGE_BLENDER")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        settings.freecad_program = vars
            .get("FORGE_FREECAD")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        settings.freecad_gui_program = vars
            .get("FORGE_FREECAD_GUI")
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
    axum::serve(listener, router(state)).await
}
