use super::{
    input_str,
    mesh::{bounds, part_mesh, to_ascii_stl, to_scad, Part},
    paths::project_path,
    Tool, ToolContext, ToolError, ToolFuture, ToolResult,
};
use crate::jsfmt::{fixed_string, normalize_json};
use serde_json::{json, Value};

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new("3d", "cad_model", "Build a 3D part from primitive shapes, in millimetres, Z up. Writes models/<name>.stl (printable, shown in the 3D viewer), models/<name>.scad (editable OpenSCAD source) and models/<name>.json (the part list). Shapes are placed side by side; overlapping shapes merge when sliced. box: `size` [x,y,z] with its minimum corner at `at`. cylinder: `radius`, `height`, base centre at `at`. sphere: `radius`, centre at `at`.", json!({"type": "object", "properties": {"name": {"type": "string", "description": "File name without extension: letters, digits, - and _"}, "parts": {"type": "array", "minItems": 1, "items": {"type": "object", "properties": {"shape": {"type": "string", "enum": ["box", "cylinder", "sphere"]}, "size": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}, "radius": {"type": "number"}, "height": {"type": "number"}, "at": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}}, "required": ["shape"]}}}, "required": ["name", "parts"]}), cad_model)
    ]
}

fn cad_model(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let name = input_str(&input, "name")?;
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
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
        let mut document = json!({"name":name,"units":"mm","parts":parts});
        normalize_json(&mut document);
        tokio::fs::write(
            project_path(&ctx.project_dir, &files[2])?,
            serde_json::to_string_pretty(&document)? + "\n",
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
