use super::{
    exec::{report, run},
    input_str,
    paths::project_path,
    Tool, ToolContext, ToolError, ToolFuture, ToolResult,
};
use crate::Settings;
use serde_json::{json, Value};

/// Running arbitrary commands is off unless the user opts in with FORGE_ALLOW_COMMANDS=1,
/// because the agent's output then executes on their machine.
pub fn commands_allowed(settings: &Settings) -> bool {
    settings.allow_commands
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new("code", "run_command", "Run a program inside the project folder (no shell: pass the program and its arguments separately), e.g. to build or test code. Times out after 2 minutes.", json!({"type": "object", "additionalProperties": false, "properties": {"program": {"type": "string", "description": "Executable name, e.g. npm, python3, make"}, "args": {"type": "array", "items": {"type": "string"}, "description": "Arguments"}}, "required": ["program"]}), run_command)
    ]
}

fn run_command(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        if !ctx.allow_commands {
            return Err(ToolError::expected(
                "running commands is disabled; start Forge with FORGE_ALLOW_COMMANDS=1 to allow it",
            ));
        }
        let args: Vec<String> = input["args"]
            .as_array()
            .map(|v| {
                v.iter()
                    .map(|s| s.as_str().unwrap_or("").to_owned())
                    .collect()
            })
            .unwrap_or_default();
        let cwd = project_path(&ctx.project_dir, "")?;
        let r = run(input_str(&input, "program")?, &args, &cwd, 120_000, &[]).await;
        Ok(ToolResult::text(report(&r)))
    })
}
