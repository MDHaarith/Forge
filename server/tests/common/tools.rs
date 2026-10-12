use super::common::TempDir;
use forge_server::tools::{all_tools, ToolContext, ToolError, ToolResult};
use forge_server::Settings;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn app_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub fn project(prefix: &str, files: &[(&str, impl AsRef<[u8]>)]) -> TempDir {
    let dir = TempDir::new(prefix);
    for (rel, data) in files {
        let full = dir.path.join(rel);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, data).unwrap();
    }
    dir
}

pub async fn run(name: &str, input: Value, dir: &Path) -> Result<ToolResult, ToolError> {
    let tool = all_tools().into_iter().find(|t| t.name == name).unwrap();
    let settings = Settings::from_env(app_dir());
    (tool.run)(
        input,
        ToolContext::from_settings(dir.to_path_buf(), &settings),
    )
    .await
}
