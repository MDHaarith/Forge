use super::{input_str, project_path, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::{
    jsfmt::{fixed, json_number, stringify_pretty},
    tools::fit::parse_stl,
};
use cadrum::{DVec3, Solid};
use serde_json::{json, Value};
use std::fs;

pub(super) fn tool() -> Tool {
    Tool::new("3d", "part_inspect",
        "Read a project STEP/STP or STL file. Returns JSON with size in mm and volume in mm³. material is optional. Hole detection, mass and overhang checks are pending; STL holes are unknown (mesh).",
        json!({"type":"object","properties":{
            "path":{"type":"string"},"material":{"type":"string"}
        },"required":["path"]}),
        part_inspect)
}

fn part_inspect(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        tokio::task::spawn_blocking(move || inspect(input, ctx))
            .await
            .map_err(|err| ToolError::unexpected(format!("part inspect failed: {err}")))?
    })
}

fn inspect(input: Value, ctx: ToolContext) -> Result<ToolResult, ToolError> {
    let file = input_str(&input, "path")?;
    let path = project_path(&ctx.project_dir, file)?;
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let material = match input.get("material") {
        None => Value::Null,
        Some(_) => json!(input_str(&input, "material")?),
    };
    let (bbox, volume) = match extension.as_str() {
        "step" | "stp" => {
            let solids = Solid::read_step(&mut fs::File::open(path)?)
                .map_err(|err| ToolError::expected(format!("STEP import failed: {err}")))?;
            let bbox = solids
                .iter()
                .map(Solid::bounding_box)
                .reduce(|[min, max], [a, b]| [min.min(a), max.max(b)])
                .unwrap_or([DVec3::ZERO; 2]);
            (bbox, solids.iter().map(Solid::volume).sum())
        }
        "stl" => {
            let vertices = parse_stl(&fs::read(path)?)?;
            if vertices.iter().any(|n| !n.is_finite()) {
                return Err(ToolError::expected("STL vertices must be finite"));
            }
            let points: Vec<_> = vertices
                .chunks_exact(3)
                .map(|p| DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64))
                .collect();
            let bbox = points
                .iter()
                .map(|p| [*p, *p])
                .reduce(|[min, max], [a, b]| [min.min(a), max.max(b)])
                .unwrap_or([DVec3::ZERO; 2]);
            let origin = bbox[0];
            let volume = points
                .chunks_exact(3)
                .map(|p| (p[0] - origin).dot((p[1] - origin).cross(p[2] - origin)) / 6.0)
                .sum::<f64>()
                .abs();
            (bbox, volume)
        }
        _ => {
            return Err(ToolError::expected(
                "path must be a .step, .stp or .stl file",
            ))
        }
    };
    let size = (bbox[1] - bbox[0])
        .to_array()
        .map(|n| json_number(fixed(n, 3)));
    // shortcut: holes, mass and overhang are placeholders until the step 4 inspection port.
    let holes = if extension == "stl" {
        json!("unknown (mesh)")
    } else {
        json!([])
    };
    let report = json!({"file":file,"size":size,"volume":json_number(fixed(volume,3)),
        "material":material,"mass_g":null,"holes":holes,"overhang_area":0,"overhang_warning":false});
    Ok(ToolResult {
        text: stringify_pretty(&report)?,
        files: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{
        mesh::{box_mesh, to_ascii_stl},
        part::test_common::TempDir,
    };

    #[tokio::test]
    async fn inspects_ascii_stl_and_stp_and_rejects_bad_inputs() {
        let dir = TempDir::new("part-inspect");
        let ascii = to_ascii_stl("box", &box_mesh([2.0, 3.0, 4.0], [10.0, -5.0, 8.0]));
        fs::write(dir.path.join("box.stl"), ascii).unwrap();
        let ctx = || ToolContext::new(dir.path.clone(), dir.path.clone());
        let result = part_inspect(json!({"path":"box.stl","material":"PLA"}), ctx())
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.text).unwrap();
        assert_eq!(
            report,
            json!({"file":"box.stl","size":[2,3,4],"volume":24,
            "material":"PLA","mass_g":null,"holes":"unknown (mesh)",
            "overhang_area":0,"overhang_warning":false})
        );
        assert_eq!(result.text, stringify_pretty(&report).unwrap());
        assert!(result.files.is_empty());

        super::super::part_build(
            json!({"name":"box","steps":[{"op":"box","size":[2,3,4]}]}),
            ctx(),
        )
        .await
        .unwrap();
        fs::copy(dir.path.join("models/box.step"), dir.path.join("box.stp")).unwrap();
        let result = part_inspect(json!({"path":"box.stp"}), ctx())
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.text).unwrap();
        assert_eq!(report["size"], json!([2, 3, 4]));
        assert_eq!(report["volume"], 24);
        assert_eq!(report["holes"], json!([]));
        assert!(report["material"].is_null());

        for path in ["../box.stl", ".git/box.step", "box.obj"] {
            assert!(part_inspect(json!({"path":path}), ctx()).await.is_err());
        }
        fs::write(
            dir.path.join("bad.stl"),
            "solid bad\nvertex inf 0 0\nvertex 0 1 0\nvertex 0 0 1\nendsolid bad",
        )
        .unwrap();
        assert!(part_inspect(json!({"path":"bad.stl"}), ctx())
            .await
            .is_err());
    }
}
