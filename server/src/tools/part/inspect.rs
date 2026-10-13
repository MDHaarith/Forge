use super::{input_str, project_path, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::{
    jsfmt::{fixed, json_number, stringify_pretty},
    tools::fit::parse_stl,
};
use cadrum::{DVec3, Solid, Tessellation};
use serde_json::{json, Value};
use std::fs;

pub(super) fn tool() -> Tool {
    Tool::new("3d", "part_inspect",
        r#"Measure a STEP, STP or STL file in the project; writes nothing. Returns one JSON object: file, size [x,y,z] in mm, volume in mm³, material and mass_g (both null unless material is given), holes (STEP only: axis, diameter, through, depth, center, fits, e.g. "M5 clearance"; the string "unknown (mesh)" for STL), overhang_area (downward-facing area steeper than 45° from vertical, excluding the bed faces, in mm²) and overhang_warning (true when overhang_area > 1). Lengths are mm, Z up. Use it after part_build to check size, holes and printability. Limits: no drawings (use freecad_drawing for drawings while it exists), no edits, and no hole detection in meshes.
Example: {"path":"models/case.step","material":"PLA"}"#,
        json!({"type":"object", "additionalProperties": false,"properties":{
            "path":{"type":"string", "description":"Project-relative .step, .stp or .stl file"},"material":{"type":"string", "description":"Optional MATERIALS key, used for mass_g"}
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
    let _kernel = super::kernel();
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
    let (bbox, volume, holes, overhang_area) = match extension.as_str() {
        "step" | "stp" => {
            let solids = Solid::read_step(&mut fs::File::open(path)?)
                .map_err(|err| ToolError::expected(format!("STEP import failed: {err}")))?;
            let bbox = solids
                .iter()
                .map(Solid::bounding_box)
                .reduce(|[min, max], [a, b]| [min.min(a), max.max(b)])
                .unwrap_or([DVec3::ZERO; 2]);
            let mesh = Solid::mesh(
                &solids,
                Tessellation {
                    deflection_linear: 0.1,
                    deflection_angular: 0.1,
                    relative_linear: false,
                },
            )
            .map_err(|err| ToolError::expected(format!("part tessellation failed: {err}")))?;
            let area = overhangs(
                mesh.indices.chunks_exact(3).map(|t| {
                    [
                        mesh.vertices[t[0]],
                        mesh.vertices[t[1]],
                        mesh.vertices[t[2]],
                    ]
                }),
                bbox[0].z,
            );
            (
                bbox,
                solids.iter().map(Solid::volume).sum(),
                json!(super::holes::holes_in(&solids, &mesh)),
                area,
            )
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
            let area = overhangs(
                points.chunks_exact(3).map(|p| [p[0], p[1], p[2]]),
                bbox[0].z,
            );
            (bbox, volume, json!("unknown (mesh)"), area)
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
    let mass = material
        .as_str()
        .map(|material| {
            super::standards::density(material)
                .map(|rho| json_number(fixed(volume / 1000.0 * rho, 3)))
        })
        .transpose()?;
    let report = json!({"file":file,"size":size,"volume":json_number(fixed(volume,3)),
        "material":material,"mass_g":mass,"holes":holes,"overhang_area":json_number(fixed(overhang_area,3)),"overhang_warning":overhang_area > 1.0});
    Ok(ToolResult {
        text: stringify_pretty(&report)?,
        files: vec![],
    })
}

fn overhangs(triangles: impl IntoIterator<Item = [DVec3; 3]>, zmin: f64) -> f64 {
    let limit = -45.0_f64.to_radians().sin();
    triangles
        .into_iter()
        .map(|[p, q, r]| {
            let n = (q - p).cross(r - p);
            let length = n.length();
            let area = length / 2.0;
            if area >= 1e-12 && n.z / length < limit && p.z.max(q.z).max(r.z) > zmin + 1e-3 {
                area
            } else {
                0.0
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::part::test_common::TempDir;

    #[tokio::test]
    async fn inspects_hole_features_patterns_and_rounded_pockets() {
        let dir = TempDir::new("part-inspect-features");
        let ctx = || ToolContext::new(dir.path.clone(), dir.path.clone());
        for (kind, feature, expected) in [
            ("counterbore", "counterbore", json!([6.5, 3.3])),
            ("countersink", "countersink", json!(6.72)),
        ] {
            super::super::part_build(
                json!({"name":"plate","steps":[
                    {"id":"a","op":"box","size":[40,30,10]},
                    {"op":"hole","target":"a","points":{"grid":[10,10,20,10,2,2]},"kind":kind}
                ]}),
                ctx(),
            )
            .await
            .unwrap();
            let result = part_inspect(json!({"path":"models/plate.step"}), ctx())
                .await
                .unwrap();
            let report: Value = serde_json::from_str(&result.text).unwrap();
            let holes = report["holes"].as_array().unwrap();
            assert_eq!(holes.len(), 4, "{report}");
            for hole in holes {
                assert_eq!(hole[feature], expected, "{report}");
                assert_eq!(hole["diameter"], 3.4);
                assert_eq!(hole["depth"], 10);
                assert_eq!(hole["through"], true);
            }
            assert_eq!(holes[0]["center"], json!([10, 10, 10]));
            assert_eq!(holes[1]["center"], json!([30, 10, 10]));
            assert_eq!(holes[2]["center"], json!([10, 20, 10]));
        }
        for (rotation, axis, center) in [
            ([0, 90, 0], "X", json!([10, 10, -10])),
            ([-90, 0, 0], "Y", json!([10, 10, -10])),
            ([180, 0, 0], "Z", json!([10, -10, -10])),
        ] {
            super::super::part_build(
                json!({"name":"blind","steps":[
                    {"id":"a","op":"box","size":[20,20,10]},
                    {"op":"hole","target":"a","points":[[10,10]],"size":"M2.5","kind":"insert"},
                    {"op":"place","target":"a","rotate":rotation}
                ]}),
                ctx(),
            )
            .await
            .unwrap();
            let result = part_inspect(json!({"path":"models/blind.step"}), ctx())
                .await
                .unwrap();
            let report: Value = serde_json::from_str(&result.text).unwrap();
            assert_eq!(report["holes"].as_array().unwrap().len(), 1, "{report}");
            let hole = &report["holes"][0];
            assert_eq!(hole["axis"], axis);
            assert_eq!(hole["center"], center);
            assert_eq!(hole["depth"], 6.2);
            assert_eq!(hole["through"], false);
            assert_eq!(hole["fits"], "M2.5 insert");
        }
        super::super::part_build(
            json!({"name":"case","steps":[
                {"id":"e","op":"enclosure","size":[60,40,20],"radius":5},
                {"op":"standoffs","target":"e","points":{"corners":[60,40,8]},"base":2,"kind":"tap"}
            ]}),
            ctx(),
        )
        .await
        .unwrap();
        let result = part_inspect(json!({"path":"models/case.step"}), ctx())
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.text).unwrap();
        assert_eq!(report["holes"].as_array().unwrap().len(), 4, "{report}");
        for hole in report["holes"].as_array().unwrap() {
            assert_eq!(hole["fits"], "M3 tap");
            assert_eq!(hole["through"], false);
        }
        assert!(!report["overhang_warning"].as_bool().unwrap());
        assert!(part_inspect(
            json!({"path":"models/case.step","material":"invalid"}),
            ctx()
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn inspects_ascii_stl_and_stp_and_rejects_bad_inputs() {
        let dir = TempDir::new("part-inspect");
        let facet = |a: &str, b: &str, c: &str| format!("facet normal 0 0 0\nouter loop\nvertex {a}\nvertex {b}\nvertex {c}\nendloop\nendfacet\n");
        let (o, x, y, z) = ("0 0 0", "1 0 0", "0 1 0", "0 0 1");
        let ascii = format!("solid t\n{}{}{}{}endsolid t\n", facet(o, y, x), facet(o, x, z), facet(o, z, y), facet(x, y, z));
        fs::write(dir.path.join("box.stl"), ascii).unwrap();
        let ctx = || ToolContext::new(dir.path.clone(), dir.path.clone());
        let result = part_inspect(json!({"path":"box.stl","material":"PLA"}), ctx())
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.text).unwrap();
        assert_eq!(
            report,
            json!({"file":"box.stl","size":[1,1,1],"volume":0.167,
            "material":"PLA","mass_g":0,"holes":"unknown (mesh)",
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
