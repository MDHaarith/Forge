use super::{number, ops, standards::*, threads, Boolean, DVec3, Solid, ToolError};
use serde_json::{json, Value};

fn optional(step: &Value, key: &str, default: f64, index: usize) -> Result<f64, ToolError> {
    match step.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| ToolError::expected(format!("step {index}: {key} must be finite"))),
    }
}

fn positive(n: f64, key: &str, index: usize) -> Result<f64, ToolError> {
    if n > 0.0 && n.is_finite() {
        Ok(n)
    } else {
        Err(ToolError::expected(format!(
            "step {index}: {key} must be positive"
        )))
    }
}

pub(super) fn points(value: &Value, index: usize) -> Result<Vec<[f64; 2]>, ToolError> {
    let numbers = |value: &Value, count: usize| -> Result<Vec<f64>, ToolError> {
        value
            .as_array()
            .filter(|a| a.len() == count)
            .and_then(|a| {
                a.iter()
                    .map(|n| n.as_f64().filter(|n| n.is_finite()))
                    .collect()
            })
            .ok_or_else(|| ToolError::expected(format!("step {index}: invalid points")))
    };
    if let Some(grid) = value.get("grid") {
        let p = numbers(grid, 6)?;
        if p[4] < 0.0
            || p[5] < 0.0
            || p[4].fract() != 0.0
            || p[5].fract() != 0.0
            || p[4] >= usize::MAX as f64
            || p[5] >= usize::MAX as f64
        {
            return Err(ToolError::expected(format!(
                "step {index}: grid counts must be nonnegative integers"
            )));
        }
        let nx = p[4] as usize;
        let ny = p[5] as usize;
        if nx == 0 || ny == 0 {
            return Ok(vec![]);
        }
        let count = nx
            .checked_mul(ny)
            .ok_or_else(|| ToolError::expected(format!("step {index}: grid is too large")))?;
        let mut points = Vec::new();
        points
            .try_reserve_exact(count)
            .map_err(|_| ToolError::expected(format!("step {index}: grid is too large")))?;
        for j in 0..ny {
            for i in 0..nx {
                let point = [p[0] + i as f64 * p[2], p[1] + j as f64 * p[3]];
                if point.iter().any(|n| !n.is_finite()) {
                    return Err(ToolError::expected(format!("step {index}: invalid points")));
                }
                points.push(point);
            }
        }
        Ok(points)
    } else if let Some(corners) = value.get("corners") {
        let p = numbers(corners, 3)?;
        let [length, width, inset] = [p[0], p[1], p[2]];
        let points = vec![
            [inset, inset],
            [length - inset, inset],
            [length - inset, width - inset],
            [inset, width - inset],
        ];
        if points.iter().flatten().any(|n| !n.is_finite()) {
            return Err(ToolError::expected(format!("step {index}: invalid points")));
        }
        Ok(points)
    } else {
        value
            .as_array()
            .ok_or_else(|| ToolError::expected(format!("step {index}: invalid points")))?
            .iter()
            .map(|point| numbers(point, 2).map(|p| [p[0], p[1]]))
            .collect()
    }
}

fn rounded_box(size: DVec3, radius: f64, at: DVec3, index: usize) -> Result<Solid, ToolError> {
    if size.min_element() <= 0.0 || !size.is_finite() || !(at + size).is_finite() {
        return Err(ToolError::expected(format!(
            "step {index}: size must be positive with finite corners"
        )));
    }
    let solid = Solid::cube(at, at + size);
    if radius > 0.0 {
        Ok(ops::modify(
            &[solid],
            &json!({"radius":radius,"edges":"vertical"}),
            "fillet",
            index,
        )?
        .remove(0))
    } else {
        Ok(solid)
    }
}

pub(super) fn shape(step: &Value, op: &str, index: usize, at: DVec3) -> Result<Solid, ToolError> {
    let wall = positive(optional(step, "wall", 2.0, index)?, "wall", index)?;
    let radius = optional(step, "radius", 0.0, index)?;
    if radius < 0.0 {
        return Err(ToolError::expected(format!(
            "step {index}: radius must be nonnegative"
        )));
    }
    let result = if op == "enclosure" {
        let size = super::vector(step, "size", index)?;
        let floor = positive(optional(step, "floor", 2.0, index)?, "floor", index)?;
        let outer = rounded_box(size, radius, DVec3::ZERO, index)?;
        let inner = rounded_box(
            DVec3::new(size.x - 2.0 * wall, size.y - 2.0 * wall, size.z),
            (radius - wall).max(0.0),
            DVec3::new(wall, wall, floor),
            index,
        )?;
        (outer - inner).build()
    } else {
        let size = points(&json!([step["size"]]), index)?[0];
        let thickness = positive(optional(step, "thickness", 2.0, index)?, "thickness", index)?;
        let lip = positive(optional(step, "lip", 2.0, index)?, "lip", index)?;
        let gap = optional(step, "gap", 0.2, index)?;
        if gap < 0.0 {
            return Err(ToolError::expected(format!(
                "step {index}: gap must be nonnegative"
            )));
        }
        let plate = rounded_box(
            DVec3::new(size[0], size[1], thickness),
            radius,
            DVec3::ZERO,
            index,
        )?;
        let inset = wall + gap;
        let lip_shape = rounded_box(
            DVec3::new(size[0] - 2.0 * inset, size[1] - 2.0 * inset, lip),
            (radius - inset).max(0.0),
            DVec3::new(inset, inset, -lip),
            index,
        )?;
        (plate + lip_shape).build()
    };
    result
        .map(|solid| solid.translate(at))
        .map_err(|err| ToolError::expected(format!("step {index}: {op} failed: {err}")))
}

fn expression(solids: &[Solid]) -> Boolean<Solid> {
    solids
        .iter()
        .fold(Boolean::default(), |expr, solid| expr + solid)
}

fn finish(expr: Boolean<Solid>, op: &str, index: usize) -> Result<Vec<Solid>, ToolError> {
    if expr.clauses().is_empty() {
        return Ok(vec![]);
    }
    expr.build_vec()
        .map_err(|err| ToolError::expected(format!("step {index}: {op} failed: {err}")))
}

fn hole(
    solids: &[Solid],
    point: [f64; 2],
    size: &str,
    step: &Value,
    index: usize,
) -> Result<Vec<Solid>, ToolError> {
    let bbox = solids
        .iter()
        .map(Solid::bounding_box)
        .reduce(|[min, max], [a, b]| [min.min(a), max.max(b)])
        .unwrap_or([DVec3::ZERO; 2]);
    let top = bbox[1].z;
    let through = bbox[1].z - bbox[0].z + 2.0;
    let kind = step
        .get("kind")
        .map(|_| super::input_str(step, "kind"))
        .transpose()?
        .unwrap_or("clearance");
    let fit = step
        .get("fit")
        .map(|_| super::input_str(step, "fit"))
        .transpose()?
        .unwrap_or("normal");
    let mut depth = step
        .get("depth")
        .map(|_| number(step, "depth", index, false))
        .transpose()?;
    let diameter = match kind {
        "clearance" | "counterbore" | "countersink" => clearance(size, fit)?,
        "tap" => lookup(TAP_DRILL, size, "tap drill")?,
        "insert" => {
            let [diameter, min_depth] = lookup(INSERT, size, "heat-set insert")?;
            depth = Some(depth.unwrap_or(min_depth + 0.5));
            diameter
        }
        _ => return Err(ToolError::expected(format!("unknown hole kind '{kind}'"))),
    };
    let length = depth.unwrap_or(through);
    let mut tool = Solid::cylinder(diameter / 2.0, DVec3::Z * (length + 1.0))
        .translate(DVec3::new(point[0], point[1], top - length));
    if kind == "counterbore" {
        let [head, height] = lookup(SOCKET_HEAD, size, "socket head")?;
        let bore = head + if head < 13.0 { 1.0 } else { 2.0 };
        let cylinder = Solid::cylinder(bore / 2.0, DVec3::Z * (height + 0.3 + 1.0))
            .translate(DVec3::new(point[0], point[1], top - height - 0.3));
        tool = (tool + cylinder)
            .build()
            .map_err(|err| ToolError::expected(format!("step {index}: hole failed: {err}")))?;
    } else if kind == "countersink" {
        let head = lookup(COUNTERSUNK_HEAD, size, "countersunk head")?;
        let sink = (head - diameter) / 2.0;
        let cone = Solid::cone(diameter / 2.0, head / 2.0 + 1.0, DVec3::Z * (sink + 1.0))
            .translate(DVec3::new(point[0], point[1], top - sink));
        tool = (tool + cone)
            .build()
            .map_err(|err| ToolError::expected(format!("step {index}: hole failed: {err}")))?;
    }
    finish(expression(solids) - tool, "hole", index)
}

pub(super) fn modify(
    solids: &[Solid],
    step: &Value,
    op: &str,
    index: usize,
) -> Result<Vec<Solid>, ToolError> {
    let points = points(&step["points"], index)?;
    let size = step
        .get("size")
        .map(|_| super::input_str(step, "size"))
        .transpose()?
        .unwrap_or("M3");
    let mut solids = solids.to_vec();
    if op == "hole" {
        if step["kind"] == "thread" && !points.is_empty() {
            threads::dimensions(size, index)?;
            let bbox = solids
                .iter()
                .map(Solid::bounding_box)
                .reduce(|[min, max], [a, b]| [min.min(a), max.max(b)])
                .unwrap_or([DVec3::ZERO; 2]);
            let depth = step
                .get("depth")
                .map(|_| number(step, "depth", index, false))
                .transpose()?;
            let length = depth.unwrap_or(bbox[1].z - bbox[0].z + 2.0);
            let diameter = lookup(TAP_DRILL, size, "tap drill")?;
            let tap = Solid::cylinder(diameter / 2.0, DVec3::Z * (length + 1.0));
            let cutter = threads::rod(size, length + 1.0, index)?;
            let mut expr = expression(&solids);
            for [x, y] in points {
                let at = DVec3::new(x, y, bbox[1].z - length);
                expr = expr - tap.clone().translate(at) - cutter.clone().translate(at);
            }
            return finish(expr, "hole", index);
        }
        for point in points {
            solids = hole(&solids, point, size, step, index)?;
        }
        return Ok(solids);
    }
    let kind = step
        .get("kind")
        .map(|_| super::input_str(step, "kind"))
        .transpose()?
        .unwrap_or("insert");
    let hole_d = match kind {
        "insert" => lookup(INSERT, size, "heat-set insert")?[0],
        "tap" => lookup(TAP_DRILL, size, "tap drill")?,
        "clearance" => clearance(size, "normal")?,
        _ => {
            return Err(ToolError::expected(format!(
                "unknown standoff kind '{kind}'"
            )))
        }
    };
    let height = positive(optional(step, "height", 5.0, index)?, "height", index)?;
    let base = optional(step, "base", 0.0, index)?;
    let outer = (hole_d * 2.0).max(hole_d + 3.0);
    let mut expr = expression(&solids);
    for [x, y] in &points {
        expr = expr
            + Solid::cylinder(outer / 2.0, DVec3::Z * height).translate(DVec3::new(*x, *y, base));
    }
    solids = finish(expr, op, index)?;
    let depth = if kind == "insert" {
        height.min(lookup(INSERT, size, "heat-set insert")?[1] + 0.5)
    } else {
        height
    };
    for [x, y] in points {
        let tool = Solid::cylinder(hole_d / 2.0, DVec3::Z * (depth + 1.0)).translate(DVec3::new(
            x,
            y,
            base + height - depth,
        ));
        solids = finish(expression(&solids) - tool, op, index)?;
    }
    Ok(solids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threaded_holes_share_blind_cutter_from_translated_top() {
        let _kernel = super::super::kernel();
        let at = DVec3::new(3.0, 4.0, 7.0);
        let cube = Solid::cube(at, at + DVec3::new(20.0, 20.0, 5.0));
        let solids = modify(
            &[cube],
            &json!({"points":[[8,9],[18,19]],"size":"M3","kind":"thread","depth":2.5}),
            "hole",
            1,
        )
        .unwrap();
        assert_eq!(solids.len(), 1);
        let flank = 2.0 * (1.5 - 1.25) / (3.0_f64.sqrt() * 0.5);
        let area = std::f64::consts::PI
            * (1.25_f64.powi(2) * (1.0 - 0.125 - flank)
                + flank * (1.25_f64.powi(2) + 1.25 * 1.5 + 1.5_f64.powi(2)) / 3.0
                + 1.5_f64.powi(2) * 0.125);
        assert!((2000.0 - solids[0].volume() - 2.0 * 2.5 * area).abs() < 0.02);
        threads::mesh(&solids, 1).unwrap();
        assert!(solids[0].bounding_box()[0].distance(at) < 0.01);
        assert!(solids[0].bounding_box()[1].distance(at + DVec3::new(20.0, 20.0, 5.0)) < 0.01);
    }

    #[test]
    fn validates_point_patterns_and_empty_grids() {
        for value in [
            json!({"grid":[0,0,1,1,-1,2]}),
            json!({"grid":[0,0,1,1,1.5,2]}),
            json!({"grid":[0,0,1,1,1e100,2]}),
            json!({"grid":[0,0,1,1,1e10,1e10]}),
            json!({"grid":[0,0,1e308,1,3,1]}),
            json!({"corners":[1,2]}),
            json!([[0, "x"]]),
        ] {
            assert!(points(&value, 3).is_err(), "{value}");
        }
        assert!(points(&json!({"grid":[0,0,1,1,0,1e10]}), 3)
            .unwrap()
            .is_empty());
        assert_eq!(
            points(&json!({"corners":[10,20,2]}), 3).unwrap(),
            vec![[2.0, 2.0], [8.0, 2.0], [8.0, 18.0], [2.0, 18.0]]
        );
    }
}
