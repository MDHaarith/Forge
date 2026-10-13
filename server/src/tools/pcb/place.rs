use super::{Footprint, Layer, Net, Spec};

const EPS: f64 = 1e-9;

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedPart {
    pub part: usize,
    pub at: [f64; 2],
    pub rotation: f64,
    pub side: Layer,
    pub footprint: Footprint,
}

impl PlacedPart {
    /// Bottom packages mirror before rotation, as seen from the board's top.
    pub fn point(&self, local: [f64; 2]) -> [f64; 2] {
        let x = if self.side == Layer::Bottom {
            -local[0]
        } else {
            local[0]
        };
        let (s, c) = self.rotation.to_radians().sin_cos();
        [
            self.at[0] + c * x - s * local[1],
            self.at[1] + s * x + c * local[1],
        ]
    }

    pub fn courtyard(&self) -> [[f64; 2]; 4] {
        let [x0, y0, x1, y1] = self.footprint.courtyard;
        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| self.point([p[0], -p[1]]))
    }
}

fn cross(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let v = [b[0] - a[0], b[1] - a[1]];
    let length = v[0].powi(2) + v[1].powi(2);
    let t = if length == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * v[0] + (p[1] - a[1]) * v[1]) / length).clamp(0.0, 1.0)
    };
    (p[0] - a[0] - t * v[0]).hypot(p[1] - a[1] - t * v[1])
}

fn intersects(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let [ac, ad, ca, cb] = [
        cross(a, b, c),
        cross(a, b, d),
        cross(c, d, a),
        cross(c, d, b),
    ];
    if ac * ad < 0.0 && ca * cb < 0.0 {
        return true;
    }
    [(ac, c, a, b), (ad, d, a, b), (ca, a, c, d), (cb, b, c, d)]
        .iter()
        .any(|(area, p, a, b)| area.abs() < EPS && distance(*p, *a, *b) < EPS)
}

fn segment_gap(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> f64 {
    if intersects(a, b, c, d) {
        return 0.0;
    }
    distance(a, c, d)
        .min(distance(b, c, d))
        .min(distance(c, a, b))
        .min(distance(d, a, b))
}

pub(super) fn inside(p: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut hit = false;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        if distance(p, a, b) < EPS {
            return true;
        }
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            hit = !hit;
        }
    }
    hit
}

pub(super) fn validate_outline(outline: &[[f64; 2]]) -> Result<(), String> {
    let outline = if outline.first() == outline.last() && outline.len() > 1 {
        &outline[..outline.len() - 1]
    } else {
        outline
    };
    if outline.len() < 3 || outline.iter().flatten().any(|x| !x.is_finite()) {
        return Err("board.outline needs at least three finite points".into());
    }
    let mut area = 0.0;
    for i in 0..outline.len() {
        let a = outline[i];
        let b = outline[(i + 1) % outline.len()];
        if (a[0] - b[0]).hypot(a[1] - b[1]) < EPS {
            return Err("board.outline has a zero-length edge".into());
        }
        area += a[0] * b[1] - a[1] * b[0];
        for j in i + 1..outline.len() {
            if j == i + 1 || (i == 0 && j == outline.len() - 1) {
                continue;
            }
            if intersects(a, b, outline[j], outline[(j + 1) % outline.len()]) {
                return Err("board.outline must not intersect itself".into());
            }
        }
    }
    if area.abs() < EPS {
        return Err("board.outline must have positive area".into());
    }
    Ok(())
}

fn on_board(part: &PlacedPart, polygon: &[[f64; 2]], edge: f64) -> bool {
    let corners = part.courtyard();
    if corners.iter().any(|&p| !inside(p, polygon)) {
        return false;
    }
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        for j in 0..4 {
            let c = corners[j];
            let d = corners[(j + 1) % 4];
            if segment_gap(a, b, c, d) + EPS < edge {
                return false;
            }
            // A concave notch can cut through a courtyard whose four corners are all inside.
            if cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0 {
                return false;
            }
        }
    }
    true
}

pub(super) fn overlap(a: &PlacedPart, b: &PlacedPart) -> bool {
    // Through-hole courtyards also reserve the opposite side's copper area.
    if a.side != b.side
        && a.footprint.pads.iter().all(|p| p.drill.is_none())
        && b.footprint.pads.iter().all(|p| p.drill.is_none())
    {
        return false;
    }
    let a = a.courtyard();
    let b = b.courtyard();
    for rect in [&a, &b] {
        for i in 0..2 {
            let p = rect[i];
            let q = rect[i + 1];
            let axis = [q[1] - p[1], p[0] - q[0]];
            let project = |points: &[[f64; 2]; 4]| {
                points
                    .iter()
                    .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], p| {
                        let dot = p[0] * axis[0] + p[1] * axis[1];
                        [lo.min(dot), hi.max(dot)]
                    })
            };
            let [a0, a1] = project(&a);
            let [b0, b1] = project(&b);
            if a1 <= b0 + EPS || b1 <= a0 + EPS {
                return false;
            }
        }
    }
    true
}

fn clears_holes(part: &PlacedPart, spec: &Spec) -> bool {
    let corners = part.courtyard();
    spec.holes.iter().all(|h| {
        !inside(h.at, &corners)
            && (0..4).all(|i| distance(h.at, corners[i], corners[(i + 1) % 4]) + EPS >= h.d / 2.0)
    })
}

/// Keep fixed placements; pack the others near already placed net neighbours.
pub fn place(
    spec: &Spec,
    footprints: &[Footprint],
    nets: &[Net],
) -> Result<Vec<PlacedPart>, String> {
    spec.validate()?;
    if footprints.len() != spec.parts.len() {
        return Err("one footprint is required per part".into());
    }
    if nets
        .iter()
        .flat_map(|n| &n.pins)
        .any(|p| p.part >= spec.parts.len())
    {
        return Err("netlist has an unknown part".into());
    }
    let polygon = spec.board.polygon();
    let bounds = spec.board.bounds();
    let mut placed: Vec<Option<PlacedPart>> = vec![None; spec.parts.len()];
    let candidate = |index: usize, at| PlacedPart {
        part: index,
        at,
        rotation: spec.parts[index].rotation.rem_euclid(360.0),
        side: spec.parts[index].side,
        footprint: footprints[index].clone(),
    };
    for (index, p) in spec.parts.iter().enumerate() {
        if let Some(at) = p.at {
            let part = candidate(index, at);
            if !on_board(&part, &polygon, spec.rules.edge) {
                return Err(format!("{} is off the board", p.reference));
            }
            for previous in placed.iter().flatten() {
                if overlap(previous, &part) {
                    return Err(format!(
                        "parts overlap: {} and {}",
                        spec.parts[previous.part].reference, p.reference
                    ));
                }
            }
            if !clears_holes(&part, spec) {
                return Err(format!("{} overlaps a hole", p.reference));
            }
            placed[index] = Some(part);
        }
    }
    let neighbours = |index: usize, placed: &[Option<PlacedPart>]| -> Vec<[f64; 2]> {
        nets.iter()
            .filter(|n| n.pins.iter().any(|p| p.part == index))
            .flat_map(|n| &n.pins)
            .filter(|p| p.part != index)
            .filter_map(|p| placed[p.part].as_ref().map(|p| p.at))
            .collect()
    };
    let mut pending: Vec<_> = (0..spec.parts.len())
        .filter(|&i| placed[i].is_none())
        .collect();
    // shortcut: 0.25 mm packing grid, coarsened beyond 1000 cells per axis; use continuous packing when tight boards need it.
    let step = 0.25_f64
        .max((bounds[2] - bounds[0]) / 1000.0)
        .max((bounds[3] - bounds[1]) / 1000.0);
    while !pending.is_empty() {
        pending.sort_by_key(|&i| (std::cmp::Reverse(neighbours(i, &placed).len()), i));
        let index = pending.remove(0);
        let near = neighbours(index, &placed);
        let target = if near.is_empty() {
            [(bounds[0] + bounds[2]) / 2.0, (bounds[1] + bounds[3]) / 2.0]
        } else {
            near.iter().fold([0.0; 2], |a, p| {
                [
                    a[0] + p[0] / near.len() as f64,
                    a[1] + p[1] / near.len() as f64,
                ]
            })
        };
        let valid = |p: &PlacedPart| {
            on_board(p, &polygon, spec.rules.edge)
                && clears_holes(p, spec)
                && placed.iter().flatten().all(|other| !overlap(p, other))
        };
        let mut best = None;
        let mut score = f64::INFINITY;
        let center = candidate(index, target);
        if valid(&center) {
            best = Some(center);
            score = 0.0;
        }
        if best.is_none() {
            for y in 0..=((bounds[3] - bounds[1]) / step).floor() as usize {
                for x in 0..=((bounds[2] - bounds[0]) / step).floor() as usize {
                    let at = [bounds[0] + x as f64 * step, bounds[1] + y as f64 * step];
                    let d = (at[0] - target[0]).powi(2) + (at[1] - target[1]).powi(2);
                    if d >= score {
                        continue;
                    }
                    let p = candidate(index, at);
                    if valid(&p) {
                        best = Some(p);
                        score = d;
                    }
                }
            }
        }
        placed[index] =
            Some(best.ok_or_else(|| format!("{} is off the board", spec.parts[index].reference))?);
    }
    Ok(placed.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::pcb::prepare;
    use serde_json::json;

    #[test]
    fn placement_preserves_fixed_parts_and_is_deterministic() {
        let input =
            json!({"board":{"width":30,"height":20},"holes":[{"at":[0,0],"d":3.2}],"parts":[
            {"ref":"R1","kind":"resistor","footprint":"0402","at":[-5,0],"rotation":90},
            {"ref":"R2","kind":"resistor","footprint":"0402"},
            {"ref":"R3","kind":"resistor","footprint":"0402"}],"nets":{"A":["R1.1","R2.1"]}})
            .to_string();
        let a = prepare(&input).unwrap();
        let b = prepare(&input).unwrap();
        assert_eq!(a.parts, b.parts);
        assert_eq!(a.parts[0].at, [-5.0, 0.0]);
        assert_eq!(a.parts[0].rotation, 90.0);
        for (i, p) in a.parts.iter().enumerate() {
            assert!(on_board(p, &a.spec.board.polygon(), a.spec.rules.edge));
            assert!(clears_holes(p, &a.spec));
            for q in &a.parts[..i] {
                assert!(!overlap(p, q));
            }
        }
        assert!((a.parts[1].at[0] + 5.0).abs() < 3.0);
    }

    #[test]
    fn outline_rotation_edge_and_opposite_sides() {
        let mut input = json!({"board":{"outline":[[-5,-5],[5,-5],[5,5],[-5,5]]},"parts":[
            {"ref":"R1","kind":"resistor","footprint":"1206","at":[3.5,0],"rotation":90}]});
        assert!(prepare(&input.to_string()).is_ok());
        input["parts"][0]["rotation"] = json!(0);
        assert_eq!(
            prepare(&input.to_string()).unwrap_err(),
            "R1 is off the board"
        );
        input["parts"] = json!([
            {"ref":"R1","kind":"resistor","footprint":"0402","at":[0,0]},
            {"ref":"R2","kind":"resistor","footprint":"0402","at":[0,0],"side":"bottom"}]);
        assert!(prepare(&input.to_string()).is_ok());
        input["board"]["outline"] = json!([[-5, -5], [5, 5], [-5, 5], [5, -5]]);
        assert!(prepare(&input.to_string()).is_err());
    }

    #[test]
    fn concave_notch_cannot_cut_through_a_courtyard() {
        let fp = Footprint {
            pads: vec![],
            courtyard: [-2.0, -2.0, 2.0, 2.0],
            height: 1.0,
            body: [-1.0, -1.0, 1.0, 1.0],
            silkscreen: vec![],
            pin1: [0.0; 2],
        };
        let p = PlacedPart {
            part: 0,
            at: [0.0; 2],
            rotation: 0.0,
            side: Layer::Top,
            footprint: fp,
        };
        let polygon = [
            [-5.0, -5.0],
            [5.0, -5.0],
            [5.0, 5.0],
            [1.0, 5.0],
            [1.0, 0.0],
            [-1.0, 0.0],
            [-1.0, 5.0],
            [-5.0, 5.0],
        ];
        assert!(p.courtyard().iter().all(|&c| inside(c, &polygon)));
        assert!(!on_board(&p, &polygon, 0.3));
    }
}
