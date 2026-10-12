use super::{
    input_str,
    exec::{report, run},
    mesh::{bounds, part_mesh, to_ascii_stl, to_scad, Part},
    paths::project_path,
    script3d::valid_name,
    Tool, ToolContext, ToolError, ToolFuture, ToolResult,
};
use crate::jsfmt::{fixed_string, stringify_pretty};
use serde_json::{json, Value};

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new("3d", "cad_model", "Build a 3D part from primitive shapes, in millimetres, Z up. Writes models/<name>.stl (printable, shown in the 3D viewer), models/<name>.scad (editable OpenSCAD source) and models/<name>.json (the part list). Shapes are placed side by side; overlapping shapes merge when sliced. box: `size` [x,y,z] with its minimum corner at `at`. cylinder: `radius`, `height`, base centre at `at`. sphere: `radius`, centre at `at`.", json!({"type": "object", "properties": {"name": {"type": "string", "description": "File name without extension: letters, digits, - and _"}, "parts": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"shape": {"type": "string", "enum": ["box", "cylinder", "sphere"]}, "size": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "radius": {"type": "number"}, "height": {"type": "number"}, "at": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "required": ["shape"]}}}, "required": ["name", "parts"]}), cad_model),
        openscad_tool(),
    ]
}

fn cad_model(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let name = input_str(&input, "name")?;
        if !valid_name(name) {
            return Err(ToolError::expected(
                "name may only use letters, digits, - and _",
            ));
        }
        let parts = input["parts"]
            .as_array()
            .ok_or_else(|| ToolError::unexpected("input.parts must be an array"))?;
        for (i, p) in parts.iter().enumerate() {
            let shape = p["shape"].as_str().unwrap_or("");
            let missing = match shape {
                "box" if p["size"].is_null() => Some("size"),
                "cylinder"
                    if p["radius"].as_f64().unwrap_or(0.) == 0.
                        || p["height"].as_f64().unwrap_or(0.) == 0. =>
                {
                    Some("radius and height")
                }
                "sphere" if p["radius"].as_f64().unwrap_or(0.) == 0. => Some("radius"),
                _ => None,
            };
            if let Some(missing) = missing {
                return Err(ToolError::expected(format!(
                    "parts[{i}] ({shape}) needs {missing}"
                )));
            }
        }
        let parsed: Vec<Part> = serde_json::from_value(input["parts"].clone())?;
        let tris = parsed.iter().flat_map(part_mesh).collect::<Vec<_>>();
        let dir = project_path(&ctx.project_dir, "models")?;
        tokio::fs::create_dir_all(dir).await?;
        let files = vec![
            format!("models/{name}.stl"),
            format!("models/{name}.scad"),
            format!("models/{name}.json"),
        ];
        tokio::fs::write(
            project_path(&ctx.project_dir, &files[0])?,
            to_ascii_stl(name, &tris),
        )
        .await?;
        tokio::fs::write(project_path(&ctx.project_dir, &files[1])?, to_scad(&parsed)).await?;
        let document = json!({"name":name,"units":"mm","parts":parts});
        tokio::fs::write(
            project_path(&ctx.project_dir, &files[2])?,
            stringify_pretty(&document)? + "\n",
        )
        .await?;
        let b = bounds(&tris);
        let size = (0..3)
            .map(|i| fixed_string(b.max[i] - b.min[i], 2))
            .collect::<Vec<_>>()
            .join(" x ");
        Ok(ToolResult {
            text: format!(
                "built {name}: {} parts, {} triangles, overall {size} mm",
                parts.len(),
                tris.len()
            ),
            files,
        })
    })
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
