use super::route::{Board, Point, Routes};
use i_overlay::{
    core::{fill_rule::FillRule, overlay_rule::OverlayRule},
    float::single::SingleFloatOverlay,
};
use std::f64::consts::{PI, TAU};

pub type Contour = Vec<Point>;
pub type Shapes = Vec<Vec<Contour>>;

pub fn rect(at: Point, size: Point) -> Contour {
    vec![
        [at[0] - size[0] / 2.0, at[1] - size[1] / 2.0],
        [at[0] + size[0] / 2.0, at[1] - size[1] / 2.0],
        [at[0] + size[0] / 2.0, at[1] + size[1] / 2.0],
        [at[0] - size[0] / 2.0, at[1] + size[1] / 2.0],
    ]
}

pub fn circle(at: Point, r: f64) -> Contour {
    // Circumscribed clearance polygons never shrink the requested gap.
    let n = 64;
    let radius = r / (PI / n as f64).cos();
    (0..n)
        .map(|i| {
            let a = TAU * (i as f64 + 0.5) / n as f64;
            [at[0] + radius * a.cos(), at[1] + radius * a.sin()]
        })
        .collect()
}

fn rounded_rect(at: Point, size: Point, r: f64) -> Contour {
    let radius = r / (PI / 64.0).cos();
    let mut p = vec![];
    for (cx, cy, a0) in [
        (size[0] / 2.0, size[1] / 2.0, 0.0),
        (-size[0] / 2.0, size[1] / 2.0, PI / 2.0),
        (-size[0] / 2.0, -size[1] / 2.0, PI),
        (size[0] / 2.0, -size[1] / 2.0, 3.0 * PI / 2.0),
    ] {
        for i in 0..=16 {
            let a = a0 + PI / 2.0 * i as f64 / 16.0;
            p.push([at[0] + cx + radius * a.cos(), at[1] + cy + radius * a.sin()]);
        }
    }
    p
}

pub(super) fn capsule(a: Point, b: Point, r: f64) -> Contour {
    let angle = (b[1] - a[1]).atan2(b[0] - a[0]);
    let radius = r / (PI / 64.0).cos();
    let mut p = vec![];
    for (c, a0) in [(b, angle - PI / 2.0), (a, angle + PI / 2.0)] {
        for i in 0..=32 {
            let t = a0 + PI * i as f64 / 32.0;
            p.push([c[0] + radius * t.cos(), c[1] + radius * t.sin()]);
        }
    }
    p
}

pub fn signed_area(p: &[Point]) -> f64 {
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
        .sum::<f64>()
        / 2.0
}
#[cfg(test)]
pub fn area(shapes: &Shapes) -> f64 {
    shapes
        .iter()
        .map(|s| {
            signed_area(&s[0]).abs() - s[1..].iter().map(|h| signed_area(h).abs()).sum::<f64>()
        })
        .sum()
}
#[cfg(test)]
fn inside_contour(p: Point, c: &[Point]) -> bool {
    let mut inside = false;
    for (a, b) in c.iter().zip(c.iter().cycle().skip(1)) {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}
#[cfg(test)]
pub fn contains(s: &Shapes, p: Point) -> bool {
    s.iter()
        .any(|shape| shape.iter().filter(|c| inside_contour(p, c)).count() % 2 == 1)
}

/// Fill one net on one layer, keeping hole contours and removing small islands.
pub fn pour(b: &Board, r: &Routes, net: usize, layer: usize) -> Shapes {
    let mut foreign = vec![];
    let mut gaps = vec![];
    let mut reinforce = vec![];
    let mut drills = vec![];
    for (a, bb) in b.outline.iter().zip(b.outline.iter().cycle().skip(1)) {
        if b.edge > 0. {
            foreign.push(capsule(*a, *bb, b.edge));
        }
    }
    for p in &b.pads {
        if p.layers & (1 << layer) == 0 {
            continue;
        }
        let contour = |extra: f64| {
            if p.round {
                circle(p.at, p.size[0] / 2. + extra)
            } else {
                rounded_rect([0.; 2], p.size, extra)
                    .into_iter()
                    .map(|q| p.point(q))
                    .collect()
            }
        };
        if p.net != net {
            foreign.push(contour(b.clearance(net).max(b.clearance(p.net))));
        } else {
            gaps.push(contour(b.clearance(net)));
            reinforce.push(contour(0.));
            for size in [
                [p.size[0] + 2. * (b.clearance(net) + 0.15), 0.3],
                [0.3, p.size[1] + 2. * (b.clearance(net) + 0.15)],
            ] {
                reinforce.push(
                    rect([0.; 2], size)
                        .into_iter()
                        .map(|q| p.point(q))
                        .collect(),
                );
            }
        }
        if p.drill > 0. {
            drills.push(circle(p.at, p.drill / 2.));
        }
    }
    for t in &r.tracks {
        if t.layer == layer && t.net != net {
            foreign.push(capsule(
                t.a,
                t.b,
                t.width / 2. + b.clearance(net).max(b.clearance(t.net)),
            ));
        }
    }
    for t in &b.fixed {
        if t.track.layer == layer && !t.nets.contains(&net) {
            foreign.push(capsule(
                t.track.a,
                t.track.b,
                t.track.width / 2. + b.clearance(net).max(b.clearance(t.track.net)),
            ));
        }
    }
    for v in &r.vias {
        if v.net != net {
            foreign.push(circle(
                v.at,
                b.via[0] / 2. + b.clearance(net).max(b.clearance(v.net)),
            ));
        }
        drills.push(circle(v.at, b.via[1] / 2.));
    }
    for &(at, size, layers) in &b.keepouts {
        if layers & (1 << layer) != 0 {
            foreign.push(rect(at, size));
        }
    }
    foreign.extend(b.antenna_keepouts.iter().cloned());
    for &(at, d) in &b.holes {
        foreign.push(circle(at, d / 2. + b.clearance(net)));
    }
    // Mirroring bottom antennas reverses winding; normalize cuts so overlaps remain a union.
    for contour in &mut foreign {
        if signed_area(contour) < 0. {
            contour.reverse();
        }
    }
    let exclusions: Vec<_> = foreign.iter().chain(&gaps).cloned().collect();
    let clear = b
        .outline
        .overlay(&exclusions, OverlayRule::Difference, FillRule::NonZero);
    let joined = clear.overlay(&reinforce, OverlayRule::Union, FillRule::NonZero);
    let clipped = joined.overlay(&b.outline, OverlayRule::Intersect, FillRule::NonZero);
    // Subtract foreign copper again: thermal spokes must never bridge a clearance cut.
    let cuts: Vec<_> = foreign.iter().chain(&drills).cloned().collect();
    let mut result = clipped.overlay(&cuts, OverlayRule::Difference, FillRule::NonZero);
    result.retain(|shape| {
        signed_area(&shape[0]).abs() - shape[1..].iter().map(|c| signed_area(c).abs()).sum::<f64>()
            >= 1.
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thermal_spokes_clear_foreign_copper_drills_and_mirrored_keepouts() {
        use super::super::{prepare, route};
        use serde_json::json;
        let input = json!({"board":{"width":20,"height":16,"layers":4},"parts":[
            {"ref":"J1","kind":"header","footprint":"pinheader_1x2","at":[0,0]},
            {"ref":"R1","kind":"resistor","footprint":"0402","at":[4,0]}],
            "nets":{"GND":["J1.1"],"SIG":["R1.1","J1.2"]},
            "pours":[{"net":"GND","layer":"bottom"}],"holes":[{"at":[-4,0],"d":2}],
            "keepouts":[{"rect":[-5,5,3,2],"layers":["bottom"]}]});
        let mut b = route::Board::from_prepared(&prepare(&input.to_string()).unwrap()).unwrap();
        let r = route::route(&b);
        let ground = b.nets.iter().position(|n| n.name == "GND").unwrap();
        let p = b.pads.iter().find(|p| p.name == "J1.1").unwrap().clone();
        // A CW contour from a mirrored antenna overlaps the normal keepout.
        let mut mirrored = rect([-5., 5.], [3., 2.]);
        mirrored.reverse();
        b.antenna_keepouts.push(mirrored);
        for layer in [1, 2] {
            let s = pour(&b, &r, ground, layer);
            assert!(!s.is_empty());
            assert_eq!(s, pour(&b, &r, ground, layer));
            assert!(!contains(&s, p.at)); // plated drill
            let h = p.size[0] / 2. + b.clearance / 2.;
            for [x, y] in [[h, 0.], [-h, 0.], [0., h], [0., -h]] {
                assert!(
                    contains(&s, [p.at[0] + x, p.at[1] + y]),
                    "thermal spoke missing"
                );
            }
            assert!(!contains(
                &s,
                [p.at[0] + h / 2f64.sqrt(), p.at[1] + h / 2f64.sqrt()]
            ));
            assert!(!contains(&s, [-4., 0.]));
            assert!(!contains(&s, [-5., 5.]));
            for shape in &s {
                assert!(area(&vec![shape.clone()]) >= 1.);
            }
            for t in r
                .tracks
                .iter()
                .filter(|t| t.layer == layer && t.net != ground)
            {
                assert!(!contains(
                    &s,
                    [(t.a[0] + t.b[0]) / 2., (t.a[1] + t.b[1]) / 2.]
                ));
            }
        }
        let mut island = b.clone();
        island.outline = rect([0.; 2], [2.; 2]);
        island.edge = 0.6;
        island.pads.clear();
        assert!(pour(&island, &r, ground, 1).is_empty());
    }

    #[test]
    fn overlapping_cuts_form_union_and_keep_holes() {
        let board = rect([0.0, 0.0], [10.0, 10.0]);
        let cuts = vec![rect([0.0, 0.0], [2.0, 2.0]), rect([1.0, 0.0], [2.0, 2.0])];
        let s = board.overlay(&cuts, OverlayRule::Difference, FillRule::NonZero);
        assert!((area(&s) - 94.0).abs() < 1e-6);
        assert!(!contains(&s, [0.5, 0.0]));
        assert!(contains(&s, [4.0, 4.0]));
        assert!(circle([0.0, 0.0], 1.0)
            .iter()
            .all(|p| super::super::route::distance(*p, [0.0, 0.0]) >= 1.0));
    }
}
