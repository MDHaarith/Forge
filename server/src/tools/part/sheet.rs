use super::{number, DVec3, Solid, ToolError, Value};
use cadrum::{DQuat, Edge};

pub(super) fn shape(step: &Value, index: usize, at: DVec3) -> Result<(Solid, [f64; 2]), ToolError> {
    let mismatch = || {
        ToolError::expected(format!(
            "step {index}: sheet needs one more flange than bends"
        ))
    };
    let flanges = step["flanges"].as_array().ok_or_else(mismatch)?;
    let bends = step["bends"].as_array().ok_or_else(mismatch)?;
    if flanges.is_empty() || flanges.len() != bends.len() + 1 {
        return Err(mismatch());
    }
    let thickness = number(step, "thickness", index, false)?;
    let width = number(step, "width", index, false)?;
    let k = step
        .get("k_factor")
        .map_or(Some(0.44), Value::as_f64)
        .filter(|k| k.is_finite())
        .ok_or_else(|| ToolError::expected(format!("step {index}: k_factor must be finite")))?;
    let failed = |err| ToolError::expected(format!("step {index}: sheet failed: {err}"));
    if !(at + DVec3::Y * width + DVec3::Z * thickness).is_finite() {
        return Err(failed("sheet corners must be finite".to_owned()));
    }
    let mut bottom = DVec3::ZERO;
    let mut top = DVec3::Z * thickness;
    let mut tangent = DVec3::X;
    let mut normal = DVec3::Z;
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    let mut flat = 0.0;
    for (i, flange) in flanges.iter().enumerate() {
        let length = flange
            .as_f64()
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or_else(|| ToolError::expected(format!("step {index}: flange must be positive")))?;
        let next_bottom = bottom + tangent * length;
        let next_top = top + tangent * length;
        if !(at + next_bottom).is_finite() || !(at + next_top).is_finite() {
            return Err(failed("flange corners must be finite".to_owned()));
        }
        lower.push(Edge::line(bottom, next_bottom).map_err(|err| failed(err.to_string()))?);
        upper.push(Edge::line(next_top, top).map_err(|err| failed(err.to_string()))?);
        bottom = next_bottom;
        top = next_top;
        flat += length;
        if let Some(bend) = bends.get(i) {
            let angle = bend["angle"]
                .as_f64()
                .filter(|a| a.is_finite() && a.abs() < 360.0)
                .ok_or_else(|| {
                    failed(
                        "bend angle must be finite and less than 360 degrees in magnitude"
                            .to_owned(),
                    )
                })?
                .to_radians();
            let radius = if bend.get("radius").is_some() {
                number(bend, "radius", index, false)?
            } else {
                thickness
            };
            if angle != 0.0 {
                let center = if angle > 0.0 {
                    top + normal * radius
                } else {
                    bottom - normal * radius
                };
                let rotate = DQuat::from_axis_angle(-DVec3::Y, angle);
                let half = DQuat::from_axis_angle(-DVec3::Y, angle / 2.0);
                let next_bottom = center + rotate * (bottom - center);
                let next_top = center + rotate * (top - center);
                let mid_bottom = center + half * (bottom - center);
                let mid_top = center + half * (top - center);
                if !center.is_finite()
                    || !(at + next_bottom).is_finite()
                    || !(at + next_top).is_finite()
                    || !(at + mid_bottom).is_finite()
                    || !(at + mid_top).is_finite()
                {
                    return Err(failed("bend corners must be finite".to_owned()));
                }
                lower.push(
                    Edge::arc_3pts(bottom, mid_bottom, next_bottom)
                        .map_err(|err| failed(err.to_string()))?,
                );
                upper.push(
                    Edge::arc_3pts(next_top, mid_top, top)
                        .map_err(|err| failed(err.to_string()))?,
                );
                bottom = next_bottom;
                top = next_top;
                tangent = rotate * tangent;
                normal = rotate * normal;
            }
            flat += angle.abs() * (radius + k * thickness);
        }
    }
    if !flat.is_finite() {
        return Err(failed("flat length must be finite".to_owned()));
    }
    lower.push(Edge::line(bottom, top).map_err(|err| failed(err.to_string()))?);
    lower.extend(upper.into_iter().rev());
    lower.push(
        Edge::line(DVec3::Z * thickness, DVec3::ZERO).map_err(|err| failed(err.to_string()))?,
    );
    // shortcut: no flat-pattern DXF or holes in the flat, add when a user needs laser files from sheet parts.
    Solid::extrude(lower.iter(), DVec3::Y * width)
        .map(|solid| (solid.translate(at), [flat, width]))
        .map_err(|err| failed(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn arbitrary_and_zero_bends_and_invalid_dimensions() {
        let _kernel = super::super::kernel();
        for angle in [0.0_f64, 60.0, -60.0] {
            let step = json!({"thickness":2,"width":3,"flanges":[10,8],"bends":[{"angle":angle,"radius":1}]});
            let (solid, flat) = shape(&step, 0, DVec3::ZERO).unwrap();
            let expected = 108.0 + angle.abs().to_radians() * (9.0 - 1.0) / 2.0 * 3.0;
            assert!((solid.volume() - expected).abs() < 1e-6);
            assert!((flat[0] - (18.0 + angle.abs().to_radians() * 1.88)).abs() < 1e-6);
        }
        let valid = json!({"thickness":2,"width":3,"flanges":[10,8],"bends":[{"angle":60}]});
        for (key, value) in [
            ("thickness", json!(0)),
            ("width", json!(-1)),
            ("flanges", json!([10, 0])),
            ("k_factor", json!("bad")),
            ("bends", json!([{"angle":360}])),
            ("bends", json!([{"angle":60,"radius":-1}])),
        ] {
            let mut step = valid.clone();
            step[key] = value;
            assert!(shape(&step, 0, DVec3::ZERO).is_err(), "{step}");
        }
    }
}
