use super::{number, ops, vector, Boolean, DVec3, Solid, ToolError, Value};
use cadrum::{Edge, ProfileOrient, SurfaceKind};

pub(super) fn loft(step: &Value, index: usize, at: DVec3) -> Result<Solid, ToolError> {
    let invalid = || {
        ToolError::expected(format!(
            "step {index}: loft needs at least 2 sections with increasing z"
        ))
    };
    let sections = step["sections"]
        .as_array()
        .filter(|sections| sections.len() >= 2)
        .ok_or_else(invalid)?;
    let mut previous = f64::NEG_INFINITY;
    let mut profiles = Vec::new();
    for section in sections {
        let z = section["z"]
            .as_f64()
            .filter(|z| z.is_finite() && *z > previous)
            .ok_or_else(invalid)?;
        profiles.push(
            ops::outline(section, index, false)?
                .into_iter()
                .map(|edge| edge.translate(DVec3::Z * z))
                .collect::<Vec<_>>(),
        );
        previous = z;
    }
    Solid::loft(
        profiles.iter().map(|profile| profile.iter()),
        step["ruled"].as_bool().unwrap_or(false),
    )
    .map(|solid| solid.translate(at))
    .map_err(|err| ToolError::expected(format!("step {index}: loft failed: {err}")))
}

pub(super) fn sweep(step: &Value, index: usize) -> Result<Solid, ToolError> {
    let failed = |err| ToolError::expected(format!("step {index}: sweep failed: {err}"));
    let path = &step["path"];
    let start = vector(path, "start", index)?;
    let segments = path["segments"]
        .as_array()
        .filter(|segments| !segments.is_empty())
        .ok_or_else(|| failed("path segments must be nonempty".to_owned()))?;
    let mut previous = start;
    let mut edges: Vec<Edge> = Vec::new();
    let mut arc_normals = Vec::new();
    for segment in segments {
        let (end, edge) = match (segment.get("line"), segment.get("arc")) {
            (Some(_), None) => {
                let end = vector(segment, "line", index)?;
                (end, Edge::line(previous, end))
            }
            (None, Some(_)) => {
                let end = vector(segment, "arc", index)?;
                let through = vector(segment, "through", index)?;
                let edge = Edge::arc_3pts(previous, through, end);
                arc_normals.push((through - previous).cross(end - previous).normalize());
                (end, edge)
            }
            _ => return Err(failed("path segment must be line or arc".to_owned())),
        };
        let edge = edge.map_err(|err| failed(err.to_string()))?;
        if edges.last().is_some_and(|last| {
            last.end_tangent()
                .normalize()
                .dot(edge.start_tangent().normalize())
                < 1.0_f64.to_radians().cos()
        }) {
            return Err(ToolError::expected(format!(
                "step {index}: path segments must meet tangentially; join corners with an arc"
            )));
        }
        edges.push(edge);
        previous = end;
    }
    let tangent = edges[0].start_tangent().normalize();
    let x_hint = if tangent.cross(DVec3::Y).length_squared() > 1e-6 {
        DVec3::Y
    } else {
        DVec3::X
    };
    let profile: Vec<_> = ops::outline(step, index, false)?
        .into_iter()
        .map(|edge| edge.align_z(tangent, x_hint).translate(start))
        .collect();
    let orient = if segments.len() == 1 && segments[0].get("line").is_some() {
        ProfileOrient::Fixed
    } else {
        // An up direction outside every arc plane cannot become parallel to an arc tangent.
        let up = [DVec3::Z, DVec3::Y, DVec3::X]
            .into_iter()
            .chain((1..=2 * edges.len() + 3).map(|n| {
                let n = n as f64;
                DVec3::new(1.0, n, n * n).normalize()
            }))
            .find(|up| {
                arc_normals
                    .iter()
                    .all(|normal| normal.dot(*up).abs() > 1e-10)
                    && edges.iter().all(|edge| {
                        edge.start_tangent().normalize().cross(*up).length_squared() > 1e-12
                            && edge.end_tangent().normalize().cross(*up).length_squared() > 1e-12
                    })
            })
            .ok_or_else(|| failed("no nonparallel up direction".to_owned()))?;
        ProfileOrient::Up(up)
    };
    Solid::sweep(&profile, &edges, orient).map_err(|err| failed(err.to_string()))
}

pub(super) fn shell(solids: &[Solid], step: &Value, index: usize) -> Result<Vec<Solid>, ToolError> {
    let thickness = number(step, "thickness", index, false)?;
    let default = serde_json::json!(["top"]);
    let open = step
        .get("open")
        .unwrap_or(&default)
        .as_array()
        .ok_or_else(|| {
            ToolError::expected(format!("step {index}: open must be an array of sides"))
        })?;
    let bbox = solids
        .iter()
        .map(Solid::bounding_box)
        .reduce(|[min, max], [a, b]| [min.min(a), max.max(b)])
        .unwrap_or([DVec3::ZERO; 2]);
    let mut faces = Vec::new();
    for side in open {
        let side = side.as_str().unwrap_or("");
        let (axis, extreme) = match side {
            "top" => (DVec3::Z, bbox[1].z),
            "bottom" => (-DVec3::Z, -bbox[0].z),
            "front" => (-DVec3::Y, -bbox[0].y),
            "back" => (DVec3::Y, bbox[1].y),
            "left" => (-DVec3::X, -bbox[0].x),
            "right" => (DVec3::X, bbox[1].x),
            _ => {
                return Err(ToolError::expected(format!(
                    "step {index}: shell found no {side} face"
                )))
            }
        };
        let selected: Vec<_> = solids
            .iter()
            .flat_map(Solid::iter_face)
            .filter(|face| {
                if !face
                    .surface()
                    .is_some_and(|surf| surf.kind == SurfaceKind::Plane)
                {
                    return false;
                }
                let (point, normal) = face.project(face.center());
                normal.dot(axis) > 1.0 - 1e-6 && (point.dot(axis) - extreme).abs() < 1e-6
            })
            .collect();
        if selected.is_empty() {
            return Err(ToolError::expected(format!(
                "step {index}: shell found no {side} face"
            )));
        }
        for face in selected {
            if !faces
                .iter()
                .any(|selected: &&cadrum::Face| selected.id() == face.id())
            {
                faces.push(face);
            }
        }
    }
    solids
        .iter()
        .map(|solid| {
            let result = solid
                .shell(
                    -thickness,
                    solid
                        .iter_face()
                        .filter(|face| faces.iter().any(|f| f.id() == face.id())),
                )
                .map_err(|err| ToolError::expected(format!("step {index}: shell failed: {err}")))?;
            let volume = result.volume();
            if !volume.is_finite() || volume <= 0.0 || volume >= solid.volume() {
                return Err(ToolError::expected(format!(
                    "step {index}: shell failed: thickness leaves no inward hollow solid"
                )));
            }
            Ok(result)
        })
        .collect()
}

pub(super) fn copies(
    solids: &[Solid],
    step: &Value,
    op: &str,
    index: usize,
) -> Result<Vec<Solid>, ToolError> {
    let mut copies = solids.to_vec();
    if op == "mirror" {
        let axis = match step["plane"].as_str() {
            Some("x") => DVec3::X,
            Some("y") => DVec3::Y,
            Some("z") => DVec3::Z,
            _ => {
                return Err(ToolError::expected(format!(
                    "step {index}: plane must be x, y or z"
                )))
            }
        };
        let origin = if step.get("origin").is_some() {
            vector(step, "origin", index)?
        } else {
            DVec3::ZERO
        };
        let mirrored = solids
            .iter()
            .map(|solid| solid.clone().mirror(origin, axis));
        if !step["keep"].as_bool().unwrap_or(true) {
            return Ok(mirrored.collect());
        }
        copies.extend(mirrored);
    } else {
        let (pattern, linear) = match (step.get("linear"), step.get("polar")) {
            (Some(pattern), None) => (pattern, true),
            (None, Some(pattern)) => (pattern, false),
            _ => {
                return Err(ToolError::expected(format!(
                    "step {index}: pattern needs linear or polar"
                )))
            }
        };
        let count = pattern["count"]
            .as_f64()
            .filter(|n| (2.0..=100.0).contains(n) && n.fract() == 0.0)
            .ok_or_else(|| {
                ToolError::expected(format!(
                    "step {index}: count must be an integer from 2 to 100"
                ))
            })? as u32;
        if linear {
            let movement = vector(pattern, "step", index)?;
            for n in 1..count {
                let offset = movement * n as f64;
                if !offset.is_finite() {
                    return Err(ToolError::expected(format!(
                        "step {index}: step must give finite offsets"
                    )));
                }
                copies.extend(solids.iter().map(|solid| solid.clone().translate(offset)));
            }
        } else {
            let center = match pattern.get("center") {
                Some(value) => {
                    let xy = value.as_array().filter(|xy| xy.len() == 2).ok_or_else(|| {
                        ToolError::expected(format!(
                            "step {index}: center must be two finite numbers"
                        ))
                    })?;
                    vector(
                        &serde_json::json!({"center":[xy[0],xy[1],0]}),
                        "center",
                        index,
                    )?
                }
                None => DVec3::ZERO,
            };
            let angle = match pattern.get("angle") {
                Some(value) => value.as_f64().filter(|n| n.is_finite()).ok_or_else(|| {
                    ToolError::expected(format!("step {index}: angle must be finite"))
                })?,
                None => 360.0,
            };
            let spacing =
                angle.to_radians() / if angle == 360.0 { count } else { count - 1 } as f64;
            for n in 1..count {
                copies.extend(
                    solids
                        .iter()
                        .map(|solid| solid.clone().rotate(center, DVec3::Z, spacing * n as f64)),
                );
            }
        }
    }
    if copies.is_empty() {
        return Ok(copies);
    }
    copies
        .iter()
        .fold(Boolean::<Solid>::default(), |expr, solid| expr + solid)
        .build_vec()
        .map_err(|err| ToolError::expected(format!("step {index}: {op} failed: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn loft_placement_and_vertical_sweeps() {
        let _kernel = super::super::kernel();
        let solid = loft(
            &json!({"sections":[
                {"outline":{"circle":[0,0,2]},"z":-5},
                {"outline":{"circle":[0,0,2]},"z":5}
            ]}),
            0,
            DVec3::new(10.0, 20.0, 30.0),
        )
        .unwrap();
        assert!(solid.bounding_box()[0].distance(DVec3::new(8.0, 18.0, 25.0)) < 1e-6);
        for segments in [
            json!([{"line":[10,20,40]}]),
            json!([{"line":[10,20,35]},{"line":[10,20,40]}]),
            json!([{"arc":[20,20,40],"through":[12.928932,20,37.071068]}]),
        ] {
            let solid = sweep(
                &json!({"outline":{"circle":[0,0,2]},"path":{
                    "start":[10,20,30],"segments":segments
                }}),
                0,
            )
            .unwrap();
            let length = if segments[0].get("arc").is_some() {
                5.0 * std::f64::consts::PI
            } else {
                10.0
            };
            assert!((solid.volume() - 4.0 * std::f64::consts::PI * length).abs() < 1e-5);
        }
    }

    #[test]
    fn shell_sides_and_body_extremes() {
        let _kernel = super::super::kernel();
        let cube = Solid::cube(DVec3::splat(10.0), DVec3::splat(20.0));
        for side in ["top", "bottom", "front", "back", "left", "right"] {
            let result = shell(std::slice::from_ref(&cube), &json!({"thickness":1,"open":[side]}), 0).unwrap();
            assert!((result[0].volume() - 424.0).abs() < 1e-6);
        }
        let solid = shell(
            std::slice::from_ref(&cube),
            &json!({"thickness":1,"open":["top","bottom","top"]}),
            0,
        )
        .unwrap();
        assert!((solid[0].volume() - 360.0).abs() < 1e-6);
    }

    #[test]
    fn copy_origins_partial_angles_and_overlap() {
        let _kernel = super::super::kernel();
        let cube = Solid::cube(DVec3::ZERO, DVec3::splat(10.0));
        let overlap = copies(
            &[cube],
            &json!({"linear":{"step":[5,0,0],"count":3.0}}),
            "pattern",
            0,
        )
        .unwrap();
        assert_eq!(overlap.len(), 1);
        assert!((overlap[0].volume() - 2000.0).abs() < 1e-6);
        let cylinder = Solid::cylinder(2.0, DVec3::Z * 10.0).translate(DVec3::new(15.0, 7.0, 0.0));
        let result = copies(
            std::slice::from_ref(&cylinder),
            &json!({"polar":{"count":3,"angle":180,"center":[5,7]}}),
            "pattern",
            0,
        )
        .unwrap();
        assert_eq!(result.len(), 3);
        let centers: Vec<_> = result.iter().map(Solid::center).collect();
        for center in [
            DVec3::new(15.0, 7.0, 5.0),
            DVec3::new(5.0, 17.0, 5.0),
            DVec3::new(-5.0, 7.0, 5.0),
        ] {
            assert!(centers.iter().any(|c| c.distance(center) < 1e-6));
        }
        for (plane, expected) in [
            ("x", DVec3::new(-5.0, 7.0, 5.0)),
            ("y", DVec3::new(15.0, 7.0, 5.0)),
            ("z", DVec3::new(15.0, 7.0, 1.0)),
        ] {
            let result = copies(
                std::slice::from_ref(&cylinder),
                &json!({"plane":plane,"origin":[5,7,3],"keep":false}),
                "mirror",
                0,
            )
            .unwrap();
            assert!(result[0].center().distance(expected) < 1e-6);
        }
        for count in [json!(1), json!(101), json!(2.5), json!(-2), Value::Null] {
            assert_eq!(
                copies(
                    std::slice::from_ref(&cylinder),
                    &json!({"linear":{"step":[1,0,0],"count":count}}),
                    "pattern",
                    4
                )
                .unwrap_err()
                .to_string(),
                "step 4: count must be an integer from 2 to 100"
            );
        }
        assert_eq!(
            copies(&[cylinder], &json!({"linear":{},"polar":{}}), "pattern", 4)
                .unwrap_err()
                .to_string(),
            "step 4: pattern needs linear or polar"
        );
    }
}
