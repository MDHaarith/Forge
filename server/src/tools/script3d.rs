use super::{
    exec::{run, ExecResult},
    launch::{launch_args, resolve_app, AppSpec, FlatpakSpec},
    paths::{lexical_path, project_path},
    ToolContext, ToolError,
};
use crate::jsfmt::trim;
use serde_json::Value;
use std::{path::Path, sync::LazyLock};

pub static BLENDER: LazyLock<AppSpec> = LazyLock::new(|| AppSpec {
    label: "blender".into(),
    native: vec!["blender".into()],
    flatpak: Some(FlatpakSpec {
        id: "org.blender.Blender".into(),
        commands: vec!["blender".into()],
    }),
});

// FreeCAD 1.x names the console program freecadcmd; 0.x used FreeCADCmd.
pub static FREECAD: LazyLock<AppSpec> = LazyLock::new(|| AppSpec {
    label: "freecadcmd".into(),
    native: vec![
        "freecadcmd".into(),
        "FreeCADCmd".into(),
        "freecad.cmd".into(),
    ],
    flatpak: Some(FlatpakSpec {
        id: "org.freecad.FreeCAD".into(),
        commands: vec!["freecadcmd".into(), "FreeCADCmd".into()],
    }),
});

pub fn with_override(spec: &AppSpec, program: Option<&Path>) -> AppSpec {
    let mut spec = spec.clone();
    if let Some(program) = program {
        spec.native
            .insert(0, program.to_string_lossy().into_owned());
    }
    spec
}

/// Runs a runner script in `spec`'s program. A Flatpak only gets the project (read-write) and the runners (read-only).
pub async fn run_app(
    spec: &AppSpec,
    args: Vec<String>,
    ctx: &ToolContext,
    timeout_ms: u64,
    env: Vec<(String, String)>,
) -> Result<ExecResult, ToolError> {
    let owned = spec.clone();
    let app = tokio::task::spawn_blocking(move || resolve_app(&owned))
        .await
        .map_err(|e| ToolError::unexpected(e.to_string()))?
        .ok_or_else(|| ToolError::expected(format!("{} is not installed", spec.label)))?;
    let project = project_path(&ctx.project_dir, "")?
        .to_string_lossy()
        .into_owned();
    let runners = lexical_path(&ctx.app_dir.join("server/runners"))
        .to_string_lossy()
        .into_owned();
    let launch = launch_args(&app, args, &[project], &[runners], env);
    Ok(run(
        &launch.cmd,
        &launch.args,
        &ctx.project_dir,
        timeout_ms,
        &launch.env,
    )
    .await)
}

#[derive(Debug, PartialEq)]
pub struct RunnerOutput {
    pub result: Option<Value>,
    pub error: Option<String>,
}

/// Reads the runner's FORGE_RESULT / FORGE_ERROR line out of the program's log.
pub fn parse_runner_output(r: &ExecResult) -> Result<RunnerOutput, ToolError> {
    for line in r.stdout.split('\n').rev() {
        if let Some(result) = line.strip_prefix("FORGE_RESULT ") {
            return Ok(RunnerOutput {
                result: Some(serde_json::from_str(result)?),
                error: None,
            });
        }
        if let Some(error) = line.strip_prefix("FORGE_ERROR ") {
            return Ok(RunnerOutput {
                result: None,
                error: Some(serde_json::from_str(error)?),
            });
        }
    }
    let log = format!("{}\n{}", r.stdout, r.stderr);
    let lines: Vec<_> = trim(&log).split('\n').collect();
    let log = lines[lines.len().saturating_sub(40)..].join("\n");
    Ok(RunnerOutput {
        result: None,
        error: Some(format!(
            "the program exited with code {} without a result. Last output:\n{log}",
            r.code
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_runner_markers_and_last_log_lines() {
        let r = |code, stdout: &str, stderr: &str| ExecResult {
            code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        };
        assert_eq!(
            parse_runner_output(&r(0, "noise\nFORGE_RESULT {\"a\":1}\n", "")).unwrap(),
            RunnerOutput {
                result: Some(serde_json::json!({"a":1})),
                error: None
            }
        );
        assert_eq!(
            parse_runner_output(&r(1, "FORGE_ERROR \"bad\"\n", ""))
                .unwrap()
                .error
                .as_deref(),
            Some("bad")
        );
        let error = parse_runner_output(&r(139, "x", "segfault"))
            .unwrap()
            .error
            .unwrap();
        assert!(error.contains("code 139") && error.contains("segfault"));
        assert_eq!(
            parse_runner_output(&r(1, "FORGE_RESULT {}\nFORGE_ERROR \"last\"", ""))
                .unwrap()
                .error
                .as_deref(),
            Some("last")
        );
    }
}
