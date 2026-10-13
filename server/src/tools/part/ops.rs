use super::{number, vector, DVec3, Solid, ToolError, Value};
use cadrum::{Edge, Tessellation};

fn point(value: &Value, index: usize, revolve: bool) -> Result<DVec3, ToolError> {
    let values = value.as_array().filter(|v| v.len() == 2).ok_or_else(|| {
        ToolError::expected(format!(
            "step {index}: outline point must be two finite numbers"
        ))
    })?;
    let coordinates = values
        .iter()
        .map(|v| v.as_f64().filter(|n| n.is_finite()))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            ToolError::expected(format!(
                "step {index}: outline point must be two finite numbers"
            ))
        })?;
    if revolve && coordinates[0] < 0.0 {
        return Err(ToolError::expected(format!(
            "step {index}: revolve radius must be nonnegative"
        )));
    }
    Ok(if revolve {
        DVec3::new(coordinates[0], 0.0, coordinates[1])
    } else {
        DVec3::new(coordinates[0], coordinates[1], 0.0)
    })
}

fn outline(step: &Value, index: usize, revolve: bool) -> Result<Vec<Edge>, ToolError> {
    let outline = &step["outline"];
    let failed = |err| ToolError::expected(format!("step {index}: outline failed: {err}"));
    if outline.get("circle").is_some() {
        let circle = vector(outline, "circle", index)?;
        if circle.z <= 0.0 || (revolve && circle.x < circle.z) {
            return Err(ToolError::expected(format!(
                "step {index}: circle needs a positive radius and a nonnegative revolve radius"
            )));
        }
        let center = if revolve {
            DVec3::new(circle.x, 0.0, circle.y)
        } else {
            DVec3::new(circle.x, circle.y, 0.0)
        };
        let axis = if revolve { -DVec3::Y } else { DVec3::Z };
        return Ok(vec![Edge::circle(circle.z, axis)
            .map_err(failed)?
            .translate(center)]);
    }
    let start = point(&outline["start"], index, revolve)?;
    let segments = outline["segments"]
        .as_array()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ToolError::expected(format!("step {index}: outline segments must be nonempty"))
        })?;
    let mut previous = start;
    let mut edges = Vec::new();
    for segment in segments {
        let (end, edge) = match (segment.get("line"), segment.get("arc")) {
            (Some(end), None) => {
                let end = point(end, index, revolve)?;
                (end, Edge::line(previous, end).map_err(failed)?)
            }
            (None, Some(end)) => {
                let end = point(end, index, revolve)?;
                let through = point(&segment["through"], index, revolve)?;
                (end, Edge::arc_3pts(previous, through, end).map_err(failed)?)
            }
            _ => {
                return Err(ToolError::expected(format!(
                    "step {index}: outline segment must be line or arc"
                )))
            }
        };
        if revolve && samples(&edge).iter().any(|p| p.x < -1e-7) {
            return Err(ToolError::expected(format!(
                "step {index}: revolve radius must be nonnegative"
            )));
        }
        edges.push(edge);
        previous = end;
    }
    if previous != start {
        return Err(ToolError::expected(format!(
            "step {index}: outline must close at start"
        )));
    }
    Ok(edges)
}

pub(super) fn profile_solid(
    step: &Value,
    op: &str,
    index: usize,
    at: DVec3,
) -> Result<Solid, ToolError> {
    let profile = outline(step, index, op == "revolve")?;
    let solid = if op == "extrude" {
        Solid::extrude(&profile, DVec3::Z * number(step, "height", index, false)?)
            .map(|solid| solid.translate(at))
    } else {
        let angle = if step.get("angle").is_some() {
            number(step, "angle", index, false)?
        } else {
            360.0
        };
        if angle > 360.0 {
            return Err(ToolError::expected(format!(
                "step {index}: angle must be at most 360"
            )));
        }
        Solid::revolve(&profile, DVec3::ZERO, DVec3::Z, angle.to_radians())
    };
    solid.map_err(|err| ToolError::expected(format!("step {index}: {op} failed: {err}")))
}

fn samples(edge: &Edge) -> Vec<DVec3> {
    // shortcut: cadrum exposes no curve type or edge bounds; sample to 1e-7 mm until it exposes those queries.
    edge.approximation_segments(Tessellation {
        deflection_linear: 1e-7,
        deflection_angular: 0.1,
        relative_linear: false,
    })
}

fn selected_edges<'a>(
    solid: &'a Solid,
    selector: &Value,
    index: usize,
) -> Result<Vec<&'a Edge>, ToolError> {
    let all = selector.as_str() == Some("all");
    let vertical = selector.as_str() == Some("vertical");
    let at_z = selector["at_z"].as_f64().filter(|n| n.is_finite());
    if !all && !vertical && at_z.is_none() {
        return Err(ToolError::expected(format!(
            "step {index}: edges must be all, vertical or {{at_z:number}}"
        )));
    }
    Ok(solid
        .iter_edge()
        .filter(|edge| {
            if all {
                return true;
            }
            let a = edge.start_point();
            let b = edge.end_point();
            if vertical {
                (a.x - b.x).abs() < 1e-6
                    && (a.y - b.y).abs() < 1e-6
                    && a.distance(b) > Edge::precision_distance()
                    && samples(edge).len() == 2
            } else {
                let z = at_z.unwrap();
                let points = samples(edge);
                !points.is_empty()
                    && (a.z - z).abs() < 1e-6
                    && (b.z - z).abs() < 1e-6
                    && points.iter().all(|p| (p.z - z).abs() < 1e-6)
                    && points.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max)
                        - points.iter().map(|p| p.z).fold(f64::INFINITY, f64::min)
                        < 1e-6
            }
        })
        .collect())
}

pub(super) fn modify(
    solids: &[Solid],
    step: &Value,
    op: &str,
    index: usize,
) -> Result<Vec<Solid>, ToolError> {
    if op == "place" {
        let optional = |key| match step.get(key) {
            Some(_) => vector(step, key, index),
            None => Ok(DVec3::ZERO),
        };
        let rotate = optional("rotate")?;
        let movement = optional("move")?;
        return Ok(solids
            .iter()
            .map(|solid| {
                let mut solid = solid.clone();
                for (angle, axis) in [
                    (rotate.x, DVec3::X),
                    (rotate.y, DVec3::Y),
                    (rotate.z, DVec3::Z),
                ] {
                    if angle != 0.0 {
                        solid = solid.rotate(DVec3::ZERO, axis, angle.to_radians());
                    }
                }
                solid.translate(movement)
            })
            .collect());
    }
    let amount = number(
        step,
        if op == "fillet" { "radius" } else { "size" },
        index,
        false,
    )?;
    let mut found = false;
    let result = solids
        .iter()
        .map(|solid| {
            let edges = selected_edges(solid, &step["edges"], index)?;
            if edges.is_empty() {
                return Ok(solid.clone());
            }
            found = true;
            let result = if op == "fillet" {
                solid.fillet_edges(amount, edges)
            } else {
                solid.chamfer_edges(amount, edges)
            };
            result.map_err(|_| ToolError::expected(format!("step {index}: {op} failed")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !found {
        return Err(ToolError::expected(format!("step {index}: {op} failed")));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::f64::consts::PI;

    #[test]
    fn curved_profiles_selectors_and_rotation_order() {
        let profile = json!({"height":3,"outline":{"start":[0,-2],"segments":[
            {"arc":[0,2],"through":[2,0]},{"line":[0,-2]}
        ]}});
        let at = DVec3::new(4.0, 5.0, 6.0);
        let solid = profile_solid(&profile, "extrude", 0, at).unwrap();
        assert!((solid.volume() - 6.0 * PI).abs() < 1e-6);
        let bbox = solid.bounding_box();
        assert!(bbox[0].distance(DVec3::new(4.0, 3.0, 6.0)) < 1e-6);
        assert!(bbox[1].distance(DVec3::new(6.0, 7.0, 9.0)) < 1e-6);
        assert_eq!(
            selected_edges(&solid, &json!("vertical"), 0).unwrap().len(),
            2
        );
        assert_eq!(
            selected_edges(&solid, &json!({"at_z":9}), 0).unwrap().len(),
            2
        );
        let chamfered = modify(
            &[solid],
            &json!({"size":0.2,"edges":{"at_z":9}}),
            "chamfer",
            0,
        )
        .unwrap();
        assert!(chamfered[0].volume() < 6.0 * PI);
        let untouched = Solid::cube(DVec3::splat(20.0), DVec3::splat(22.0));
        let mixed = modify(
            &[chamfered[0].clone(), untouched],
            &json!({"radius":0.1,"edges":{"at_z":6}}),
            "fillet",
            0,
        )
        .unwrap();
        assert!(mixed[0].volume() < chamfered[0].volume());
        assert!((mixed[1].volume() - 8.0).abs() < 1e-6);
        assert!(modify(
            &mixed,
            &json!({"size":0.1,"edges":{"at_z":100}}),
            "chamfer",
            0
        )
        .is_err());

        let sphere = profile_solid(&profile, "revolve", 0, DVec3::ZERO).unwrap();
        assert!((sphere.volume() - 32.0 * PI / 3.0).abs() < 1e-6);
        let torus = profile_solid(
            &json!({"outline":{"circle":[5,2,1]}}),
            "revolve",
            0,
            DVec3::ZERO,
        )
        .unwrap();
        assert!((torus.volume() - 10.0 * PI * PI).abs() < 1e-6);
        let upright = profile_solid(&profile, "extrude", 0, DVec3::ZERO)
            .unwrap()
            .rotate_y(PI / 2.0);
        assert_eq!(
            selected_edges(&upright, &json!({"at_z":0}), 0)
                .unwrap()
                .len(),
            4
        );
        assert!(selected_edges(&upright, &json!("vertical"), 0)
            .unwrap()
            .is_empty());

        let cube = Solid::cube(DVec3::new(1.0, 2.0, 3.0), DVec3::new(5.0, 8.0, 11.0));
        let moved = modify(
            &[cube],
            &json!({"rotate":[90,90,90],"move":[10,20,30]}),
            "place",
            0,
        )
        .unwrap();
        let bbox = moved[0].bounding_box();
        assert!(bbox[0].distance(DVec3::new(13.0, 22.0, 25.0)) < 1e-6);
        assert!(bbox[1].distance(DVec3::new(21.0, 28.0, 29.0)) < 1e-6);

        for invalid in [
            json!({"outline":{"start":[0,0],"segments":[{"line":[1,0]}]}}),
            json!({"outline":{"start":[0,0],"segments":[{"arc":[1,0]}]}}),
            json!({"outline":{"circle":[0,0,-1]}}),
            json!({"outline":{"circle":[0,0,1]}}),
        ] {
            assert!(profile_solid(&invalid, "revolve", 4, DVec3::ZERO).is_err());
        }
        assert!(selected_edges(&moved[0], &json!("sideways"), 4).is_err());
    }
}
