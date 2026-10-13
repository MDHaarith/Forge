use super::{input_str, paths::project_path, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use serde_json::{json, Value};
use std::path::Path;

const MAX_READ: u64 = 200_000;

/// Lists project files (relative, forward slashes), skipping .git and node_modules.
pub async fn list_files(project_dir: &Path) -> Result<Vec<String>, ToolError> {
    list_files_in(project_dir, "").await
}

fn list_files_in<'a>(
    project_dir: &'a Path,
    dir: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, ToolError>> + Send + 'a>>
{
    Box::pin(async move {
        let mut entries = tokio::fs::read_dir(project_path(project_dir, dir)?).await?;
        let mut sorted = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            sorted.push(entry);
        }
        sorted.sort_by_key(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.to_lowercase(), name)
        });
        let mut out = Vec::new();
        for entry in sorted {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" || name == "node_modules" {
                continue;
            }
            let rel = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if entry.file_type().await?.is_dir() {
                out.extend(list_files_in(project_dir, &rel).await?);
            } else {
                out.push(rel);
            }
        }
        Ok(out)
    })
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new("workspace", "list_files", "List every file in the project (paths relative to the project root).", json!({"type": "object", "additionalProperties": false, "properties": {}}), list_files_tool),
        Tool::new("workspace", "read_file", "Read a text file from the project.", json!({"type": "object", "additionalProperties": false, "properties": {"path": {"type": "string", "description": "Project-relative path"}}, "required": ["path"]}), read_file),
        Tool::new("workspace", "write_file", "Create or overwrite a text file in the project. Use it for source code, OpenSCAD, Verilog, notes, BOMs and any other text artifact. Parent folders are created.", json!({"type": "object", "additionalProperties": false, "properties": {"path": {"type": "string", "description": "Project-relative path"}, "contents": {"type": "string", "description": "The full file contents"}}, "required": ["path", "contents"]}), write_file)
    ]
}

fn list_files_tool(_input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let files = list_files(&ctx.project_dir).await?;
        Ok(ToolResult::text(if files.is_empty() {
            "(the project is empty)".into()
        } else {
            files.join("\n")
        }))
    })
}
fn read_file(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let rel = input_str(&input, "path")?;
        let full = project_path(&ctx.project_dir, rel)?;
        let stat = tokio::fs::metadata(&full)
            .await
            .ok()
            .filter(|m| m.is_file())
            .ok_or_else(|| ToolError::expected(format!("{rel} does not exist")))?;
        if stat.len() > MAX_READ {
            return Err(ToolError::expected(format!(
                "{rel} is {} bytes; only files up to {MAX_READ} bytes can be read",
                stat.len()
            )));
        }
        let data = tokio::fs::read(full).await?;
        Ok(ToolResult::text(String::from_utf8_lossy(&data)))
    })
}
fn write_file(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let rel = input_str(&input, "path")?;
        let contents = input_str(&input, "contents")?;
        let full = project_path(&ctx.project_dir, rel)?;
        tokio::fs::create_dir_all(full.parent().unwrap()).await?;
        tokio::fs::write(full, contents).await?;
        Ok(ToolResult {
            text: format!("wrote {rel} ({} bytes)", contents.len()),
            files: vec![rel.into()],
        })
    })
}
