use super::{
    input_str,
    exec::{report, run},
    paths::project_path,
    Tool, ToolContext, ToolError, ToolFuture, ToolResult,
};
use serde_json::{json, Value};

pub fn tools() -> Vec<Tool> {
    vec![openscad_tool()]
}

fn openscad_tool() -> Tool {
    let mut tool = Tool::new("3d", "openscad_render", r#"Render an OpenSCAD file in the project to STL with the real OpenSCAD engine (supports difference(), hull(), etc.)."#, json!({"type":"object","properties":{"path":{"type":"string","description":"Project-relative .scad file"},"output":{"type":"string","description":"Project-relative .stl to write (default: same name)"}},"required":["path"]}), openscad_render);
    tool.requires = vec!["openscad".into()];
    tool.install = Some(serde_json::from_value(json!({"dnf":"openscad","apt":"openscad","brew":"--cask openscad","url":"https://openscad.org/downloads.html"})).unwrap());
    tool
}

fn openscad_render(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let src = input_str(&input, "path")?;
        let out = input.get("output").filter(|v| !v.is_null()).map(|v| v.as_str().unwrap().to_owned()).unwrap_or_else(|| {
            let base = if src.to_lowercase().ends_with(".scad") { &src[..src.len() - 5] } else { src };
            format!("{base}.stl")
        });
        project_path(&ctx.project_dir, src)?;
        project_path(&ctx.project_dir, &out)?;
        let r = run("openscad", &["-o".into(), out.clone(), src.into()], &ctx.project_dir, 120_000, &[]).await;
        if r.code != 0 { return Err(ToolError::expected(report(&r))); }
        Ok(ToolResult { text: format!("rendered {out}\n{}", report(&r)), files: vec![out] })
    })
}
