use crate::route::{Board, Point, Routes};
use cadrum::{DVec3, Edge, Solid};
use i_overlay::{
    core::{fill_rule::FillRule, overlay_rule::OverlayRule},
    float::single::SingleFloatOverlay,
};
use std::{
    error::Error,
    f64::consts::{PI, TAU},
    time::Instant,
};

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

fn capsule(a: Point, b: Point, r: f64) -> Contour {
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
pub fn area(shapes: &Shapes) -> f64 {
    shapes
        .iter()
        .map(|s| {
            signed_area(&s[0]).abs() - s[1..].iter().map(|h| signed_area(h).abs()).sum::<f64>()
        })
        .sum()
}
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
pub fn contains(s: &Shapes, p: Point) -> bool {
    s.iter()
        .any(|shape| shape.iter().filter(|c| inside_contour(p, c)).count() % 2 == 1)
}

struct Input {
    outline: Contour,
    foreign: Vec<Contour>,
    gaps: Vec<Contour>,
    reinforce: Vec<Contour>,
    drills: Vec<Contour>,
    thermal: Vec<(Point, Point, bool)>,
}
fn input(b: &Board, r: &Routes) -> Input {
    let ground = b.nets.iter().position(|n| n.name == "GND").unwrap();
    let mut a = Input {
        outline: rect(
            [0.0, 0.0],
            [b.size[0] - 2.0 * b.edge, b.size[1] - 2.0 * b.edge],
        ),
        foreign: vec![],
        gaps: vec![],
        reinforce: vec![],
        drills: vec![],
        thermal: vec![],
    };
    for p in &b.pads {
        if p.layers & 2 == 0 {
            continue;
        }
        let expanded = if p.round {
            circle(p.at, p.size[0] / 2.0 + b.clearance)
        } else {
            rounded_rect(p.at, p.size, b.clearance)
        };
        if p.net != ground {
            a.foreign.push(expanded);
        } else {
            a.gaps.push(expanded);
            a.reinforce.push(if p.round {
                circle(p.at, p.size[0] / 2.0)
            } else {
                rect(p.at, p.size)
            });
            let width = 0.3;
            a.reinforce
                .push(rect(p.at, [p.size[0] + 2.0 * (b.clearance + 0.15), width]));
            a.reinforce
                .push(rect(p.at, [width, p.size[1] + 2.0 * (b.clearance + 0.15)]));
            a.thermal.push((p.at, p.size, p.round));
        }
    }
    for t in &r.tracks {
        if t.layer == 1 && t.net != ground {
            a.foreign
                .push(capsule(t.a, t.b, t.width / 2.0 + b.clearance));
        }
    }
    for v in &r.vias {
        if v.net != ground {
            a.foreign.push(circle(v.at, b.via[0] / 2.0 + b.clearance));
        }
    }
    for &(at, size, layers) in &b.keepouts {
        if layers & 2 != 0 {
            a.foreign.push(rect(at, size));
        }
    }
    for &(at, d) in &b.holes {
        a.foreign.push(circle(at, d / 2.0 + b.clearance));
    }
    for p in &b.pads {
        if p.drill > 0.0 {
            a.drills.push(circle(p.at, p.drill / 2.0));
        }
    }
    for v in &r.vias {
        a.drills.push(circle(v.at, b.via[1] / 2.0));
    }
    a
}

fn overlay(a: &Input) -> Shapes {
    let exclusions: Vec<_> = a.foreign.iter().chain(&a.gaps).cloned().collect();
    let clear = a
        .outline
        .overlay(&exclusions, OverlayRule::Difference, FillRule::NonZero);
    let joined = clear.overlay(&a.reinforce, OverlayRule::Union, FillRule::NonZero);
    let clipped = joined.overlay(&a.outline, OverlayRule::Intersect, FillRule::NonZero);
    let cuts: Vec<_> = a.foreign.iter().chain(&a.drills).cloned().collect();
    clipped.overlay(&cuts, OverlayRule::Difference, FillRule::NonZero)
}

fn slab(p: &Contour) -> Result<Solid, Box<dyn Error>> {
    let profile = Edge::polygon(
        &p.iter()
            .map(|p| DVec3::new(p[0], p[1], 0.0))
            .collect::<Vec<_>>(),
    )?;
    Ok(Solid::extrude(&profile, DVec3::Z * 0.035)?)
}

fn kernel(a: &Input) -> Result<(Vec<Solid>, usize), Box<dyn Error>> {
    let mut polygons = vec![a.outline.clone()];
    polygons.extend(a.foreign.iter().cloned());
    let gap_start = polygons.len();
    polygons.extend(a.gaps.iter().cloned());
    let reinforce_start = polygons.len();
    polygons.extend(a.reinforce.iter().cloned());
    let drills_start = polygons.len();
    polygons.extend(a.drills.iter().cloned());
    let solids = polygons.iter().map(slab).collect::<Result<Vec<_>, _>>()?;
    let mut clauses = vec![];
    // DNF: (board minus foreign and gaps) union (each core/spoke clipped to board minus foreign).
    let mut append = |reinforce: Option<usize>| {
        clauses.push(1);
        for i in 1..gap_start {
            clauses.push(-((i + 1) as i64));
        }
        if let Some(i) = reinforce {
            clauses.push((i + 1) as i64);
        } else {
            for i in gap_start..reinforce_start {
                clauses.push(-((i + 1) as i64));
            }
        }
        for i in drills_start..solids.len() {
            clauses.push(-((i + 1) as i64));
        }
        clauses.push(0);
    };
    append(None);
    for i in reinforce_start..drills_start {
        append(Some(i));
    }
    Ok((Solid::boolean(&solids, clauses).build_vec()?, solids.len()))
}

pub fn benchmark(b: &Board, r: &Routes, log: &mut String) -> Result<Shapes, Box<dyn Error>> {
    let t = Instant::now();
    let face = Edge::polygon(
        &rect([0.0, 0.0], b.size)
            .iter()
            .map(|p| DVec3::new(p[0], p[1], 0.0))
            .collect::<Vec<_>>(),
    )?;
    let reason = match Solid::extrude(&face, DVec3::ZERO) {
        Ok(_) => "unexpected zero-thickness solid".to_string(),
        Err(e) => e.to_string(),
    };
    crate::report(log,format!("pour cadrum 2D attempt: Face has no public constructor/boolean; zero-thickness extrude='{reason}'; time_ms={:.3}",t.elapsed().as_secs_f64()*1000.0));
    let a = input(b, r);
    let t = Instant::now();
    let kernel_result = kernel(&a);
    let kernel_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let mut result = overlay(&a);
    let before = result.len();
    result.retain(|shape| area(&vec![shape.clone()]) >= 1.0);
    let polygon_ms = t.elapsed().as_secs_f64() * 1000.0;
    let poly_area = area(&result);
    let mut samples: Vec<_> = (0..30)
        .flat_map(|x| (0..20).map(move |y| [-14.73 + x as f64, -9.71 + y as f64]))
        .collect();
    let mut thermal_checks = 0;
    for &(at, size, round) in &a.thermal {
        let half = size[0] / 2.0;
        for (p, want) in [
            ([at[0] + half + b.clearance / 2.0, at[1]], true),
            ([at[0] - half - b.clearance / 2.0, at[1]], true),
            ([at[0], at[1] + size[1] / 2.0 + b.clearance / 2.0], true),
            ([at[0], at[1] - size[1] / 2.0 - b.clearance / 2.0], true),
            (
                if round {
                    [
                        at[0] + (half + b.clearance / 2.0) / 2.0_f64.sqrt(),
                        at[1] + (half + b.clearance / 2.0) / 2.0_f64.sqrt(),
                    ]
                } else {
                    [
                        at[0] + half + b.clearance / 3.0,
                        at[1] + size[1] / 2.0 + b.clearance / 3.0,
                    ]
                },
                false,
            ),
        ] {
            assert_eq!(contains(&result, p), want, "thermal relief at {p:?}");
            samples.push(p);
            thermal_checks += 1;
        }
    }
    for c in &a.foreign {
        let centre = [
            c.iter().map(|p| p[0]).sum::<f64>() / c.len() as f64,
            c.iter().map(|p| p[1]).sum::<f64>() / c.len() as f64,
        ];
        assert!(!contains(&result, centre));
    }
    for &(at, _) in &b.holes {
        assert!(!contains(&result, at));
    }
    let mut mismatches = 0;
    match kernel_result {
        Ok((solids, count)) => {
            let kernel_area: f64 = solids.iter().map(|s| s.volume() / 0.035).sum();
            let top_area: f64 = solids
                .iter()
                .flat_map(|s| s.iter_face())
                .filter(|f| {
                    f.center().z > 0.035 - 1e-8
                        && f.surface().is_some_and(|s| s.axis_z.z.abs() > 0.999)
                })
                .map(|f| f.area())
                .sum();
            for p in &samples {
                if solids
                    .iter()
                    .any(|s| s.contains(DVec3::new(p[0], p[1], 0.0175)))
                    != contains(&result, *p)
                {
                    mismatches += 1;
                }
            }
            crate::report(log,format!("pour cadrum thin-solid proxy: operands={count} components={} thickness=0.035 mm area={kernel_area:.6} mm2 top_face_area={top_area:.6} mm2 time_ms={kernel_ms:.3} above_1s={} vs_polygon_area_delta={:.9} mm2 sample_mismatches={mismatches}/{}",solids.len(),kernel_ms>1000.0,(kernel_area-poly_area).abs(),samples.len()));
            assert!((kernel_area - poly_area).abs() < 1e-4);
            assert!((kernel_area - top_area).abs() < 1e-6);
        }
        Err(e) => crate::report(
            log,
            format!("pour cadrum thin-solid proxy failed: {e}; time_ms={kernel_ms:.3}"),
        ),
    }
    assert_eq!(mismatches, 0);
    crate::report(log,format!("pour i_overlay 4.5.2: area={poly_area:.6} mm2 components={} removed_islands={} holes={} clearance={} mm thermal_pads={} spokes={} spoke_width=0.3 mm thermal_point_checks={thermal_checks} time_ms={polygon_ms:.3}",result.len(),before-result.len(),result.iter().map(|s|s.len()-1).sum::<usize>(),b.clearance,a.thermal.len(),a.thermal.len()*4));
    crate::report(log,"polygon choice: i_overlay; public planar booleans, contour output, native holes/islands, no artificial thickness; cadrum remains STEP/GLB kernel".into());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
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
            .all(|p| crate::route::distance(*p, [0.0, 0.0]) >= 1.0));
    }
}
