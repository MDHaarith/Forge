use super::{
    exec::{clip, run, ExecResult},
    launch::{launch_args, resolve_app, AppSpec, FlatpakSpec},
    paths::{lexical_path, project_path},
    Tool, ToolContext, ToolError, ToolFuture, ToolResult,
};
use crate::jsfmt::{string, trim, truthy};
use serde_json::{json, Value};
use std::{path::Path, sync::LazyLock};

pub static BLENDER: LazyLock<AppSpec> = LazyLock::new(|| AppSpec {
    label: "blender".into(),
    native: vec!["blender".into()],
    flatpak: Some(FlatpakSpec {
        id: "org.blender.Blender".into(),
        commands: vec!["blender".into()],
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
    let program = ctx.blender_program.as_deref();
    let owned = with_override(spec, program);
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

pub fn tools() -> Vec<Tool> {
    vec![
        {
            let mut tool = Tool::new("3d", "blender_build", r#"Run a Blender Python (bpy) script headless to build visual 3D: product renders, organic or sculpted shapes, scenes, materials, artwork, game assets. Not for parts that need exact engineering dimensions (use part_build).
Write the script with write_file first. It runs in an empty scene with the project folder as working directory; create objects, modifiers, materials, and optionally lights and a camera. Afterwards every object is exported to models/<name>.glb (shown in Forge's 3D viewer) plus any other requested formats. With render=true a 960×720 PNG is rendered to renders/<name>.png with Cycles on the CPU; if the script adds no camera or lights, Forge frames the scene and lights it. Blender units are metres by default."#, json!({"type":"object", "additionalProperties": false,"properties":{"script":{"type":"string","description":"Project-relative .py file written beforehand with write_file"},"name":{"type":"string","description":"Base name for output files: letters, digits, - and _"},"formats":{"type":"array","items":{"type":"string","enum":["glb","stl","obj","blend"]},"description":"Export formats (default [\"glb\"]). stl for 3D printing, blend to keep an editable Blender file."},"render":{"type":"boolean","description":"Also render a PNG (takes longer)"}},"required":["script","name"]}), blender_build);
            tool.app = Some(BLENDER.clone());
            tool.install = Some(serde_json::from_value(json!({"dnf":"blender python3-numpy","apt":"blender python3-numpy","brew":"--cask blender","flatpak":"org.blender.Blender","url":"https://www.blender.org/download/"})).unwrap());
            tool
        },
    ]
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

pub fn joined(value: &Value, separator: &str) -> String {
    value.as_array().unwrap().iter().map(|v| if v.is_null() { String::new() } else { string(Some(v)) }).collect::<Vec<_>>().join(separator)
}

pub fn files(value: &Value) -> Vec<String> {
    value.as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_owned()).collect()
}

async fn check_inputs(input: &Value, ctx: &ToolContext, allowed: &[&str]) -> Result<(String, String, Vec<String>), ToolError> {
    let name = string(input.get("name"));
    if !valid_name(&name) { return Err(ToolError::expected("name may only use letters, digits, - and _")); }
    let script = super::input_str(input, "script")?;
    let full = project_path(&ctx.project_dir, script)?;
    if !tokio::fs::metadata(&full).await.is_ok_and(|m| m.is_file()) {
        return Err(ToolError::expected(format!("{script} does not exist; write it with write_file first")));
    }
    let formats = match input.get("formats").filter(|v| !v.is_null()) {
        Some(value) => files(value),
        None => allowed.iter().map(|s| (*s).into()).collect(),
    };
    let unknown: Vec<_> = formats.iter().filter(|f| !allowed.contains(&f.as_str())).cloned().collect();
    if !unknown.is_empty() { return Err(ToolError::expected(format!("unsupported formats: {}", unknown.join(", ")))); }
    Ok((name, full.to_string_lossy().into_owned(), formats))
}

fn blender_build(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let (name, script, formats) = check_inputs(&input, &ctx, &["glb", "stl", "obj", "blend"]).await?;
        let chosen = if input.get("formats").is_some_and(truthy) { formats } else { vec!["glb".into()] };
        let render = truthy(&input["render"]);
        let r = run_app(&BLENDER, vec!["--background".into(), "--factory-startup".into(), "--python-exit-code".into(), "1".into(), "--python".into(), ctx.app_dir.join("server/runners/blender_runner.py").to_string_lossy().into_owned()], &ctx, if render { 15 * 60_000 } else { 5 * 60_000 }, vec![
            ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
            ("FORGE_PROJECT".into(), project_path(&ctx.project_dir, "")?.to_string_lossy().into_owned()),
            ("FORGE_SCRIPT".into(), script), ("FORGE_NAME".into(), name),
            ("FORGE_FORMATS".into(), chosen.join(",")), ("FORGE_RENDER".into(), if render { "1" } else { "0" }.into()),
        ]).await?;
        let parsed = parse_runner_output(&r)?;
        if let Some(error) = parsed.error { return Err(ToolError::expected(clip(&format!("Blender: {error}"), 12_000))); }
        let res = parsed.result.unwrap();
        let files = files(&res["files"]);
        Ok(ToolResult { text: format!("Blender built {} objects ({} vertices, {} faces), overall {} units.\nFiles: {}", res["objects"].as_array().unwrap().len(), string(res.get("vertices")), string(res.get("faces")), joined(&res["size"], " × "), files.join(", ")), files })
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
