pub mod cad;
pub mod code;
pub mod exec;
pub mod launch;
pub mod mesh;
pub mod paths;
pub mod script3d;
pub mod validate;
pub mod vcs;
pub mod workspace;

use crate::{
    platform::{current_distro, install_text, InstallHint},
    Settings,
};
use launch::{resolve_app, AppSpec};
use serde_json::{json, Value};
use std::{future::Future, path::PathBuf, pin::Pin};

/// The workshop areas a tool belongs to. The UI groups capabilities by these.
pub type Domain = &'static str;

#[derive(Clone, Debug)]
pub struct ToolContext {
    /// Absolute path of the open project. Tools must not touch anything outside it.
    pub project_dir: PathBuf,
    pub app_dir: PathBuf,
    pub allow_commands: bool,
}

impl ToolContext {
    pub fn new(project_dir: PathBuf, app_dir: PathBuf) -> Self {
        Self {
            project_dir: paths::lexical_path(&project_dir),
            app_dir: paths::lexical_path(&app_dir),
            allow_commands: false,
        }
    }
    pub fn from_settings(project_dir: PathBuf, settings: &Settings) -> Self {
        let mut context = Self::new(project_dir, settings.app_dir.clone());
        context.allow_commands = settings.allow_commands;
        context
    }
}

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub text: String,
    /// Project-relative files the tool created or changed, so the UI can refresh and preview them.
    pub files: Vec<String>,
}
impl ToolResult {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            files: vec![],
        }
    }
}

pub type ToolFuture = Pin<Box<dyn Future<Output = Result<ToolResult, ToolError>> + Send>>;

/// A capability Forge can hand to the agent. Every domain plugin (CAD, PCB, chip, ...)
/// is a list of these. Tools that wrap an external program name it in `requires`;
/// they are hidden from the agent until that program is on PATH.
#[derive(Clone)]
pub struct Tool {
    pub domain: Domain,
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    /// Program(s) the tool runs; the first one found is used. Empty for built-in tools.
    pub requires: Vec<String>,
    /// A desktop program that may also be installed as a Flatpak; replaces `requires`.
    pub app: Option<AppSpec>,
    /// What the UI says is missing when `requires` lists file paths rather than program names.
    pub needs: Option<&'static str>,
    pub install: Option<InstallHint>,
    pub run: fn(Value, ToolContext) -> ToolFuture,
}
impl Tool {
    pub fn new(
        domain: Domain,
        name: &'static str,
        description: &'static str,
        input_schema: Value,
        run: fn(Value, ToolContext) -> ToolFuture,
    ) -> Self {
        Self {
            domain,
            name,
            description,
            input_schema,
            run,
            requires: vec![],
            app: None,
            needs: None,
            install: None,
        }
    }
}

/// Thrown by tools for expected failures; the message is shown to the agent as-is.
#[derive(Debug)]
pub enum ToolError {
    Expected(String),
    Unexpected(String),
}
impl ToolError {
    pub fn expected(message: impl Into<String>) -> Self {
        Self::Expected(message.into())
    }
    pub fn unexpected(message: impl Into<String>) -> Self {
        Self::Unexpected(message.into())
    }
}
impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Expected(m) | Self::Unexpected(m) => f.write_str(m),
        }
    }
}
impl std::error::Error for ToolError {}
impl From<std::io::Error> for ToolError {
    fn from(e: std::io::Error) -> Self {
        Self::unexpected(e.to_string())
    }
}
impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        Self::unexpected(e.to_string())
    }
}

/// Every capability Forge knows about. Add a domain by appending its tool list here.
pub fn all_tools() -> Vec<Tool> {
    let mut tools = workspace::tools();
    tools.extend(vcs::tools());
    tools.extend(code::tools());
    tools.extend(cad::tools());
    // phase 2: append script3d, freecad, board, rf, design, pcb, chip (and openscad_render).
    tools
}

pub fn is_available(tool: &Tool, settings: &Settings) -> bool {
    if tool.name == "run_command" {
        return code::commands_allowed(settings);
    }
    if let Some(app) = &tool.app {
        return resolve_app(app).is_some();
    }
    if tool.requires.is_empty() {
        return true;
    }
    tool.requires.iter().any(|program| {
        if program.contains(['/', '\\']) && !std::path::Path::new(program).is_absolute() {
            exec::on_path(&settings.app_dir.join(program).to_string_lossy())
        } else {
            exec::on_path(program)
        }
    })
}

/// The tools the agent may call right now: those whose external programs are installed.
pub fn available_tools(settings: &Settings) -> Vec<Tool> {
    all_tools()
        .into_iter()
        .filter(|tool| is_available(tool, settings))
        .collect()
}

pub fn describe_tools(settings: &Settings) -> Vec<Value> {
    all_tools().iter().map(|t| {
        let mut description = json!({"name":t.name, "domain":t.domain, "description":t.description, "available":is_available(t,settings)});
        let requires = if t.name == "run_command" {Some("FORGE_ALLOW_COMMANDS=1")}
            else {t.needs.or_else(|| t.app.as_ref().map(|app| app.label.as_str())).or_else(|| t.requires.last().map(String::as_str))};
        if let Some(requires) = requires { description["requires"] = json!(requires); }
        if let Some(install) = install_text(t.install.as_ref(),current_distro()) { description["install"] = json!(install); }
        description
    }).collect()
}

pub fn input_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    input[key]
        .as_str()
        .ok_or_else(|| ToolError::unexpected(format!("input.{key} must be a string")))
}
