use super::{input_str, project_path, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::{
    jsfmt::{fixed, json_number, stringify_pretty},
    tools::fit::parse_stl,
};
use cadrum::{Boolean, DVec3, Mesh, Solid, Tessellation};
use serde_json::{json, Value};
use std::fs;

pub(super) fn tool() -> Tool {
    Tool::new(
        "3d",
        "part_inspect",
        r#"Measure a STEP, STP or STL file in the project; writes nothing. Returns one JSON object: file, size [x,y,z] in mm, volume in mm³, center_of_mass [x,y,z], material and mass_g (both null unless material is given), holes (STEP only: axis, diameter, through, depth, center, fits, e.g. "M5 clearance"; the string "unknown (mesh)" for STL), overhang_area (downward-facing area steeper than overhang degrees from vertical, default 45°, excluding the bed faces, in mm²), overhang_warning (true when overhang_area > 1), and interference. Optional against lists other project-relative STEP/STP files. Interference reports every pair of solids in path and against, labelled <file>#<n> counting from 1 per file, with a, b, overlap_volume in mm³ and clearance in mm (0 when touching or overlapping). Clearance uses 0.01 mm tessellations; interference is always [] for STL. Size, volume, center of mass, holes and overhangs describe path only. Lengths are mm, Z up. Use it after part_build to check size, holes, printability and fit between parts. Limits: no drawings, no edits, and no hole detection in meshes.
Example: {"path":"models/case.step","material":"PLA","against":["models/lid.step"],"overhang":45}"#,
        json!({"type":"object", "additionalProperties": false,"properties":{
            "path":{"type":"string", "description":"Project-relative .step, .stp or .stl file"},"material":{"type":"string", "description":"Optional MATERIALS key, used for mass_g"},
            "against":{"type":"array","items":{"type":"string"},"description":"Other project-relative .step or .stp files for interference and clearance"},
            "overhang":{"type":"number","description":"Overhang angle from vertical, from 0 to 90 degrees (default 45)"}
        },"required":["path"]}),
        part_inspect,
    )
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
    let overhang = match input.get("overhang") {
        None => 45.0,
        Some(value) => value
            .as_f64()
            .filter(|n| n.is_finite() && (0.0..=90.0).contains(n))
            .ok_or_else(|| ToolError::expected("overhang must be between 0 and 90"))?,
    };
    let mut against = Vec::new();
    if let Some(value) = input.get("against") {
        for value in value
            .as_array()
            .ok_or_else(|| ToolError::expected("against must be an array of paths"))?
        {
            let file = value
                .as_str()
                .ok_or_else(|| ToolError::expected("against files must be .step or .stp"))?;
            let path = project_path(&ctx.project_dir, file)?;
            if !path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("step") || s.eq_ignore_ascii_case("stp"))
            {
                return Err(ToolError::expected("against files must be .step or .stp"));
            }
            against.push((file, path));
        }
    }
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let material = match input.get("material") {
        None => Value::Null,
        Some(_) => json!(input_str(&input, "material")?),
    };
    let (bbox, volume, center, holes, overhang_area, interference) = match extension.as_str() {
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
                overhang,
            );
            let volume: f64 = solids.iter().map(Solid::volume).sum();
            let center = if volume == 0.0 {
                DVec3::ZERO
            } else {
                solids
                    .iter()
                    .map(|s| s.center() * s.volume())
                    .sum::<DVec3>()
                    / volume
            };
            let holes = json!(super::holes::holes_in(&solids, &mesh));
            let mut labelled: Vec<_> = solids
                .into_iter()
                .enumerate()
                .map(|(i, solid)| (format!("{file}#{}", i + 1), solid))
                .collect();
            for (file, path) in against {
                let solids = Solid::read_step(&mut fs::File::open(path)?)
                    .map_err(|err| ToolError::expected(format!("STEP import failed: {err}")))?;
                labelled.extend(
                    solids
                        .into_iter()
                        .enumerate()
                        .map(|(i, solid)| (format!("{file}#{}", i + 1), solid)),
                );
            }
            let interference = interference(&labelled)?;
            (bbox, volume, center, holes, area, interference)
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
            let mut volume = 0.0;
            let mut moment = DVec3::ZERO;
            for p in points.chunks_exact(3) {
                let [a, b, c] = [p[0] - origin, p[1] - origin, p[2] - origin];
                let v = a.dot(b.cross(c)) / 6.0;
                volume += v;
                moment += (a + b + c) * (v / 4.0);
            }
            let center = if volume == 0.0 {
                DVec3::ZERO
            } else {
                origin + moment / volume
            };
            let area = overhangs(
                points.chunks_exact(3).map(|p| [p[0], p[1], p[2]]),
                bbox[0].z,
                overhang,
            );
            (
                bbox,
                volume.abs(),
                center,
                json!("unknown (mesh)"),
                area,
                vec![],
            )
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
        "center_of_mass":center.to_array().map(|n| json_number(fixed(n,3))),
        "material":material,"mass_g":mass,"holes":holes,"overhang_area":json_number(fixed(overhang_area,3)),"overhang_warning":overhang_area > 1.0,"interference":interference});
    Ok(ToolResult {
        text: stringify_pretty(&report)?,
        files: vec![],
    })
}

fn interference(solids: &[(String, Solid)]) -> Result<Vec<Value>, ToolError> {
    if solids.len() < 2 {
        return Ok(vec![]);
    }
    let meshes = solids
        .iter()
        .map(|(_, solid)| {
            Solid::mesh(
                [solid],
                Tessellation {
                    deflection_linear: 0.01,
                    deflection_angular: 0.1,
                    relative_linear: false,
                },
            )
            .map(|mesh| triangles(&mesh))
            .map_err(|err| ToolError::expected(format!("part tessellation failed: {err}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut pairs = Vec::new();
    for (i, (a, solid_a)) in solids.iter().enumerate() {
        for (j, (b, solid_b)) in solids.iter().enumerate().skip(i + 1) {
            let overlap = if bbox_distance_squared(solid_a.bounding_box(), solid_b.bounding_box())
                > 0.0
            {
                0.0
            } else {
                (Boolean::from(solid_a) * Boolean::from(solid_b))
                    .build_vec()
                    .map_err(|err| ToolError::expected(format!("part intersection failed: {err}")))?
                    .iter()
                    .map(Solid::volume)
                    .sum()
            };
            let clearance = if overlap > 0.0 {
                0.0
            } else {
                mesh_clearance(&meshes[i], &meshes[j])
            };
            pairs.push(
                json!({"a":a,"b":b,"overlap_volume":json_number(fixed(overlap,3)),
                "clearance":json_number(fixed(clearance,3))}),
            );
        }
    }
    Ok(pairs)
}

struct Triangle {
    points: [DVec3; 3],
    bbox: [DVec3; 2],
}

fn triangles(mesh: &Mesh) -> Vec<Triangle> {
    mesh.indices
        .chunks_exact(3)
        .map(|t| {
            let points = [
                mesh.vertices[t[0]],
                mesh.vertices[t[1]],
                mesh.vertices[t[2]],
            ];
            Triangle {
                points,
                bbox: [
                    points[0].min(points[1]).min(points[2]),
                    points[0].max(points[1]).max(points[2]),
                ],
            }
        })
        .collect()
}

fn bbox_distance_squared([amin, amax]: [DVec3; 2], [bmin, bmax]: [DVec3; 2]) -> f64 {
    (amin - bmax)
        .max(bmin - amax)
        .max(DVec3::ZERO)
        .length_squared()
}

fn point_segment_distance_squared(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() == 0.0 {
        0.0
    } else {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    };
    p.distance_squared(a + t * ab)
}

fn point_triangle_distance_squared(p: DVec3, [a, b, c]: [DVec3; 3]) -> f64 {
    let normal = (b - a).cross(c - a);
    if normal.length_squared() > 0.0 {
        let projected = p - normal * (p - a).dot(normal) / normal.length_squared();
        if [(a, b), (b, c), (c, a)]
            .iter()
            .all(|(a, b)| (b - a).cross(projected - a).dot(normal) >= 0.0)
        {
            return p.distance_squared(projected);
        }
    }
    [(a, b), (b, c), (c, a)]
        .into_iter()
        .map(|(a, b)| point_segment_distance_squared(p, a, b))
        .fold(f64::INFINITY, f64::min)
}

fn segment_distance_squared(p: DVec3, q: DVec3, a: DVec3, b: DVec3) -> f64 {
    let u = q - p;
    let v = b - a;
    let w = p - a;
    let uu = u.length_squared();
    let vv = v.length_squared();
    if uu == 0.0 {
        return point_segment_distance_squared(p, a, b);
    }
    if vv == 0.0 {
        return point_segment_distance_squared(a, p, q);
    }
    let uv = u.dot(v);
    let uw = u.dot(w);
    let vw = v.dot(w);
    let denominator = uu * vv - uv * uv;
    let mut s = if denominator > 0.0 {
        ((uv * vw - uw * vv) / denominator).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut t = (uv * s + vw) / vv;
    if t < 0.0 {
        t = 0.0;
        s = (-uw / uu).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((uv - uw) / uu).clamp(0.0, 1.0);
    }
    (p + s * u).distance_squared(a + t * v)
}

fn mesh_clearance(a: &[Triangle], b: &[Triangle]) -> f64 {
    // shortcut: clearance comes from tessellations (exact for flat faces, within 0.01 mm on curves), use BRepExtrema when cadrum exposes it.
    let mut best = f64::INFINITY;
    for a in a {
        for b in b {
            if bbox_distance_squared(a.bbox, b.bbox) >= best {
                continue;
            }
            for p in a.points {
                best = best.min(point_triangle_distance_squared(p, b.points));
            }
            for p in b.points {
                best = best.min(point_triangle_distance_squared(p, a.points));
            }
            for i in 0..3 {
                for j in 0..3 {
                    best = best.min(segment_distance_squared(
                        a.points[i],
                        a.points[(i + 1) % 3],
                        b.points[j],
                        b.points[(j + 1) % 3],
                    ));
                }
            }
            if best == 0.0 {
                return 0.0;
            }
        }
    }
    best.sqrt()
}

fn overhangs(triangles: impl IntoIterator<Item = [DVec3; 3]>, zmin: f64, angle: f64) -> f64 {
    let limit = -angle.to_radians().sin();
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

    #[test]
    fn clearance_includes_crossed_edges_and_degenerate_segments() {
        let triangle = |points: [DVec3; 3]| Triangle {
            points,
            bbox: [
                points[0].min(points[1]).min(points[2]),
                points[0].max(points[1]).max(points[2]),
            ],
        };
        let a = triangle([
            DVec3::new(-2.0, -1.0, 0.0),
            DVec3::new(2.0, -1.0, 0.0),
            DVec3::new(0.0, 2.0, 0.0),
        ]);
        let b = triangle([
            DVec3::new(-2.0, 1.0, 1.0),
            DVec3::new(2.0, 1.0, 1.0),
            DVec3::new(0.0, -2.0, 1.0),
        ]);
        for p in a.points {
            assert!(point_triangle_distance_squared(p, b.points) > 1.0);
        }
        for p in b.points {
            assert!(point_triangle_distance_squared(p, a.points) > 1.0);
        }
        assert!((mesh_clearance(&[a], &[b]) - 1.0).abs() < 1e-12);
        for (p, q, a, b, expected) in [
            (-DVec3::X, DVec3::X, -DVec3::Y, DVec3::Y, 0.0),
            (
                -DVec3::X,
                DVec3::X,
                -DVec3::Y + DVec3::Z,
                DVec3::Y + DVec3::Z,
                1.0,
            ),
            (DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::X + DVec3::Y, 1.0),
            (DVec3::ZERO, DVec3::ZERO, DVec3::X, DVec3::X, 1.0),
            (DVec3::ZERO, DVec3::X, DVec3::X * 2.0, DVec3::X * 3.0, 1.0),
        ] {
            assert!((segment_distance_squared(p, q, a, b) - expected).abs() < 1e-12);
            assert!((segment_distance_squared(a, b, p, q) - expected).abs() < 1e-12);
        }
        assert_eq!(
            point_triangle_distance_squared(DVec3::Y, [DVec3::ZERO, DVec3::X, DVec3::X * 2.0]),
            1.0
        );
        let down = [[DVec3::Z, DVec3::Z + DVec3::Y, DVec3::Z + DVec3::X]];
        assert_eq!(overhangs(down, 0.0, 0.0), 0.5);
        assert_eq!(overhangs(down, 0.0, 90.0), 0.0);
        assert_eq!(overhangs(down, 1.0, 0.0), 0.0);
        let slope = [[
            DVec3::Z,
            DVec3::Z + DVec3::Y,
            DVec3::new(1.0, 0.0, 1.0 + 3.0_f64.sqrt()),
        ]];
        assert!((overhangs(slope, 0.0, 20.0) - 1.0).abs() < 1e-12);
        assert_eq!(overhangs(slope, 0.0, 40.0), 0.0);
    }

    #[tokio::test]
    async fn inspects_all_pairs_and_keeps_primary_mass_properties() {
        let dir = TempDir::new("part-inspect-pairs");
        let ctx = || ToolContext::new(dir.path.clone(), dir.path.clone());
        for (name, steps) in [
            (
                "primary",
                json!([
                    {"op":"box","size":[2,2,2]},
                    {"op":"box","size":[4,2,2],"at":[10,0,0]}
                ]),
            ),
            (
                "other",
                json!([
                    {"op":"box","size":[2,2,2],"at":[14,0,0]},
                    {"op":"box","size":[2,2,2],"at":[30,0,0]}
                ]),
            ),
        ] {
            super::super::part_build(json!({"name":name,"steps":steps}), ctx())
                .await
                .unwrap();
        }
        let result = part_inspect(
            json!({"path":"models/primary.step","against":["models/other.step"]}),
            ctx(),
        )
        .await
        .unwrap();
        let report: Value = serde_json::from_str(&result.text).unwrap();
        assert_eq!(report["center_of_mass"], json!([8.333, 1, 1]));
        assert_eq!(report["volume"], 24);
        assert_eq!(report["size"], json!([14, 2, 2]));
        assert_eq!(
            report
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "file",
                "size",
                "volume",
                "center_of_mass",
                "material",
                "mass_g",
                "holes",
                "overhang_area",
                "overhang_warning",
                "interference"
            ]
        );
        let pairs = report["interference"].as_array().unwrap();
        let labels = [
            "models/primary.step#1",
            "models/primary.step#2",
            "models/other.step#1",
            "models/other.step#2",
        ];
        let mut index = 0;
        for (i, a) in labels.iter().enumerate() {
            for b in labels.iter().skip(i + 1) {
                assert_eq!(pairs[index]["a"], *a);
                assert_eq!(pairs[index]["b"], *b);
                assert_eq!(pairs[index]["overlap_volume"], 0);
                assert_eq!(
                    pairs[index]
                        .as_object()
                        .unwrap()
                        .keys()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    ["a", "b", "overlap_volume", "clearance"]
                );
                index += 1;
            }
        }
        assert_eq!(pairs.len(), index);
        assert_eq!(pairs[3]["clearance"], 0);
        let primary = part_inspect(json!({"path":"models/primary.step"}), ctx())
            .await
            .unwrap();
        let primary: Value = serde_json::from_str(&primary.text).unwrap();
        assert_eq!(primary["interference"].as_array().unwrap().len(), 1);
        for file in ["../other.step", "/other.step", ".git/other.step"] {
            assert!(part_inspect(
                json!({"path":"models/primary.step","against":[file]}),
                ctx()
            )
            .await
            .is_err());
        }
    }

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
        let facet = |a: &str, b: &str, c: &str| {
            format!("facet normal 0 0 0\nouter loop\nvertex {a}\nvertex {b}\nvertex {c}\nendloop\nendfacet\n")
        };
        let (o, x, y, z) = ("0 0 0", "1 0 0", "0 1 0", "0 0 1");
        let ascii = format!(
            "solid t\n{}{}{}{}endsolid t\n",
            facet(o, y, x),
            facet(o, x, z),
            facet(o, z, y),
            facet(x, y, z)
        );
        fs::write(dir.path.join("box.stl"), ascii).unwrap();
        let ctx = || ToolContext::new(dir.path.clone(), dir.path.clone());
        let result = part_inspect(json!({"path":"box.stl","material":"PLA"}), ctx())
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.text).unwrap();
        assert_eq!(
            report,
            json!({"file":"box.stl","size":[1,1,1],"volume":0.167,"center_of_mass":[0.25,0.25,0.25],
            "material":"PLA","mass_g":0,"holes":"unknown (mesh)",
            "overhang_area":0,"overhang_warning":false,"interference":[]})
        );
        assert_eq!(result.text, stringify_pretty(&report).unwrap());
        assert!(result.files.is_empty());
        let (o, x, y, z) = ("10 20 30", "11 20 30", "10 21 30", "10 20 31");
        let reversed = format!(
            "solid t\n{}{}{}{}endsolid t\n",
            facet(o, x, y),
            facet(o, z, x),
            facet(o, y, z),
            facet(x, z, y)
        );
        fs::write(dir.path.join("translated.stl"), reversed).unwrap();
        let result = part_inspect(json!({"path":"translated.stl","overhang":90}), ctx())
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&result.text).unwrap();
        assert_eq!(report["center_of_mass"], json!([10.25, 20.25, 30.25]));
        assert_eq!(report["volume"], 0.167);
        assert_eq!(report["overhang_area"], 0);

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
        assert_eq!(report["center_of_mass"], json!([1, 1.5, 2]));
        assert_eq!(report["interference"], json!([]));
        for angle in [-1, 91] {
            assert_eq!(
                part_inspect(json!({"path":"box.stp","overhang":angle}), ctx())
                    .await
                    .unwrap_err()
                    .to_string(),
                "overhang must be between 0 and 90"
            );
        }

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
