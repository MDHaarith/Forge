use super::{
    exec::{report, run},
    input_str,
    paths::project_path,
    Tool, ToolContext, ToolError, ToolFuture, ToolResult,
};
use crate::jsfmt::trim;
use crate::platform::InstallHint;
use serde_json::{json, Value};
use std::path::Path;

/// Commits as Forge when the user has no git identity configured, so commits never fail on a fresh machine.
async fn identity(cwd: &Path) -> Vec<String> {
    let email = run(
        "git",
        &["config".into(), "user.email".into()],
        cwd,
        120_000,
        &[],
    )
    .await;
    if email.code == 0 && !trim(&email.stdout).is_empty() {
        vec![]
    } else {
        vec![
            "-c".into(),
            "user.name=Forge".into(),
            "-c".into(),
            "user.email=forge@localhost".into(),
        ]
    }
}

pub fn tools() -> Vec<Tool> {
    let mut tools = vec![
        Tool::new(
            "vcs",
            "git_status",
            "Show which project files changed since the last commit.",
            json!({"type": "object", "additionalProperties": false, "properties": {}}),
            git_status,
        ),
        Tool::new("vcs", "git_commit", "Record every current change in the project's history with a message. Commit after each meaningful step so the user can review and roll back.", json!({"type": "object", "additionalProperties": false, "properties": {"message": {"type": "string", "description": "What changed and why, one line"}}, "required": ["message"]}), git_commit),
        Tool::new(
            "vcs",
            "git_log",
            "Show the most recent commits in the project.",
            json!({"type": "object", "additionalProperties": false, "properties": {"limit": {"type": "integer", "description": "How many commits (default 20)"}}}),
            git_log,
        ),
    ];
    let install: InstallHint = serde_json::from_value(json!({"dnf": "git", "apt": "git", "brew": "git"})).unwrap();
    for tool in &mut tools {
        tool.requires = vec!["git".into()];
        tool.install = Some(install.clone());
    }
    tools
}

fn git_status(_input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let cwd = project_path(&ctx.project_dir, "")?;
        let r = run(
            "git",
            &["status".into(), "--short".into()],
            &cwd,
            120_000,
            &[],
        )
        .await;
        Ok(ToolResult::text(if r.code == 0 {
            if trim(&r.stdout).is_empty() {
                "working tree clean".into()
            } else {
                trim(&r.stdout).into()
            }
        } else {
            report(&r)
        }))
    })
}
fn git_commit(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let cwd = project_path(&ctx.project_dir, "")?;
        run("git", &["add".into(), "-A".into()], &cwd, 120_000, &[]).await;
        let staged = run(
            "git",
            &["diff".into(), "--cached".into(), "--quiet".into()],
            &cwd,
            120_000,
            &[],
        )
        .await;
        if staged.code == 0 {
            return Ok(ToolResult::text("nothing to commit"));
        }
        let mut args = identity(&cwd).await;
        args.extend([
            "commit".into(),
            "-m".into(),
            input_str(&input, "message")?.into(),
        ]);
        let r = run("git", &args, &cwd, 120_000, &[]).await;
        if r.code != 0 {
            return Err(ToolError::expected(report(&r)));
        }
        let head = run(
            "git",
            &["log".into(), "-1".into(), "--format=%h %s".into()],
            &cwd,
            120_000,
            &[],
        )
        .await;
        Ok(ToolResult::text(format!(
            "committed {}",
            trim(&head.stdout)
        )))
    })
}
fn git_log(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let limit = input["limit"].as_f64().unwrap_or(20.).clamp(1., 200.);
        let cwd = project_path(&ctx.project_dir, "")?;
        let r = run(
            "git",
            &[
                "log".into(),
                format!("-{}", crate::jsfmt::number(limit)),
                "--format=%h %ad %s".into(),
                "--date=short".into(),
            ],
            &cwd,
            120_000,
            &[],
        )
        .await;
        Ok(ToolResult::text(if r.code == 0 {
            if trim(&r.stdout).is_empty() {
                "no commits yet".into()
            } else {
                trim(&r.stdout).into()
            }
        } else {
            report(&r)
        }))
    })
}
/// Creates the repository for a new project with an initial commit.
pub async fn init_repo(project_dir: &Path) -> Result<(), ToolError> {
    let cwd = project_path(project_dir, "")?;
    run("git", &["init".into(), "-q".into()], &cwd, 120_000, &[]).await;
    run("git", &["add".into(), "-A".into()], &cwd, 120_000, &[]).await;
    let mut args = identity(&cwd).await;
    args.extend([
        "commit".into(),
        "-q".into(),
        "-m".into(),
        "Start project".into(),
    ]);
    run("git", &args, &cwd, 120_000, &[]).await;
    Ok(())
}
