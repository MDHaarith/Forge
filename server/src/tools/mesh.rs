//! A small mesh kernel so Forge can produce printable 3D parts with no CAD program installed.
//! Parts are placed side by side (no boolean operations); each part also gets an OpenSCAD
//! equivalent so the design can be refined in real CAD later.

use crate::jsfmt::{fixed_string, number};
use serde::{Deserialize, Serialize};

pub type Vec3 = [f64; 3];
pub type Triangle = [Vec3; 3];

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum Part {
    Box {
        size: Vec3,
        #[serde(default)]
        at: Vec3,
    },
    Cylinder {
        radius: f64,
        height: f64,
        #[serde(default)]
        at: Vec3,
    },
    Sphere {
        radius: f64,
        #[serde(default)]
        at: Vec3,
    },
}

const SEGMENTS: usize = 32;
fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Axis-aligned box with its minimum corner at `at`.
pub fn box_mesh(size: Vec3, at: Vec3) -> Vec<Triangle> {
    let [x, y, z] = size;
    let v = |i: f64, j: f64, k: f64| add(at, [i * x, j * y, k * z]);
    let quads = [
        [v(0., 0., 0.), v(0., 1., 0.), v(1., 1., 0.), v(1., 0., 0.)], // bottom (-z)
        [v(0., 0., 1.), v(1., 0., 1.), v(1., 1., 1.), v(0., 1., 1.)], // top (+z)
        [v(0., 0., 0.), v(1., 0., 0.), v(1., 0., 1.), v(0., 0., 1.)], // front (-y)
        [v(0., 1., 0.), v(0., 1., 1.), v(1., 1., 1.), v(1., 1., 0.)], // back (+y)
        [v(0., 0., 0.), v(0., 0., 1.), v(0., 1., 1.), v(0., 1., 0.)], // left (-x)
        [v(1., 0., 0.), v(1., 1., 0.), v(1., 1., 1.), v(1., 0., 1.)], // right (+x)
    ];
    quads
        .into_iter()
        .flat_map(|[a, b, c, d]| [[a, b, c], [a, c, d]])
        .collect()
}

/// Upright cylinder whose base centre sits at `at`.
pub fn cylinder(radius: f64, height: f64, at: Vec3) -> Vec<Triangle> {
    let ring = |z| {
        (0..SEGMENTS)
            .map(|i| {
                let t = (i as f64 / SEGMENTS as f64) * std::f64::consts::PI * 2.;
                add(at, [radius * t.cos(), radius * t.sin(), z])
            })
            .collect::<Vec<_>>()
    };
    let bottom = ring(0.);
    let top = ring(height);
    let cb = add(at, [0., 0., 0.]);
    let ct = add(at, [0., 0., height]);
    let mut tris = Vec::new();
    for i in 0..SEGMENTS {
        let j = (i + 1) % SEGMENTS;
        tris.extend([
            [cb, bottom[j], bottom[i]],
            [ct, top[i], top[j]],
            [bottom[i], bottom[j], top[j]],
            [bottom[i], top[j], top[i]],
        ]);
    }
    tris
}

/// UV sphere centred on `at`.
pub fn sphere(radius: f64, at: Vec3) -> Vec<Triangle> {
    let rows = SEGMENTS / 2;
    let point = |r: usize, c: usize| {
        let phi = (r as f64 / rows as f64) * std::f64::consts::PI;
        let theta = (c as f64 / SEGMENTS as f64) * std::f64::consts::PI * 2.;
        add(
            at,
            [
                radius * phi.sin() * theta.cos(),
                radius * phi.sin() * theta.sin(),
                radius * phi.cos(),
            ],
        )
    };
    let mut tris = Vec::new();
    for r in 0..rows {
        for c in 0..SEGMENTS {
            let (a, b, d, e) = (
                point(r, c),
                point(r + 1, c),
                point(r, c + 1),
                point(r + 1, c + 1),
            );
            if r > 0 {
                tris.push([a, b, d]);
            }
            if r < rows - 1 {
                tris.push([d, b, e]);
            }
        }
    }
    tris
}

pub fn part_mesh(part: &Part) -> Vec<Triangle> {
    match part {
        Part::Box { size, at } => box_mesh(*size, *at),
        Part::Cylinder { radius, height, at } => cylinder(*radius, *height, *at),
        Part::Sphere { radius, at } => sphere(*radius, *at),
    }
}

fn normal([a, b, c]: Triangle) -> Vec3 {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = n[0].hypot(n[1]).hypot(n[2]);
    let len = if len == 0. { 1. } else { len };
    [n[0] / len, n[1] / len, n[2] / len]
}
fn coords(p: Vec3) -> String {
    p.map(|n| fixed_string(n, 5)).join(" ")
}

pub fn to_ascii_stl(name: &str, tris: &[Triangle]) -> String {
    let mut lines = vec![format!("solid {name}")];
    for tri in tris {
        lines.extend([
            format!("  facet normal {}", coords(normal(*tri))),
            "    outer loop".into(),
        ]);
        for p in tri {
            lines.push(format!("      vertex {}", coords(*p)));
        }
        lines.extend(["    endloop".into(), "  endfacet".into()]);
    }
    lines.push(format!("endsolid {name}"));
    lines.join("\n") + "\n"
}

pub fn to_scad(parts: &[Part]) -> String {
    let body = parts
        .iter()
        .map(|p| {
            let (at, shape) = match p {
                Part::Box { size, at } => (at, format!("cube([{}]);", size.map(number).join(", "))),
                Part::Cylinder { radius, height, at } => (
                    at,
                    format!(
                        "cylinder(r = {}, h = {}, $fn = {SEGMENTS});",
                        number(*radius),
                        number(*height)
                    ),
                ),
                Part::Sphere { radius, at } => (
                    at,
                    format!("sphere(r = {}, $fn = {SEGMENTS});", number(*radius)),
                ),
            };
            format!("  translate([{}]) {shape}", at.map(number).join(", "))
        })
        .collect::<Vec<_>>();
    format!("// Generated by Forge. Edit freely and render with the openscad_render tool.\nunion() {{\n{}\n}}\n",body.join("\n"))
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}
pub fn bounds(tris: &[Triangle]) -> Bounds {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for tri in tris {
        for p in tri {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
    }
    Bounds { min, max }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn box_is_twelve_triangles_spanning_size() {
        let tris = box_mesh([10., 20., 30.], [1., 2., 3.]);
        assert_eq!(tris.len(), 12);
        assert_eq!(
            bounds(&tris),
            Bounds {
                min: [1., 2., 3.],
                max: [11., 22., 33.]
            }
        );
    }
    #[test]
    fn cylinder_and_sphere_stay_inside_radius() {
        let c = bounds(&cylinder(5., 10., [0., 0., 2.]));
        assert!(
            (c.max[0] - 5.).abs() < 1e-9
                && (c.min[2] - 2.).abs() < 1e-9
                && (c.max[2] - 12.).abs() < 1e-9
        );
        let s = bounds(&sphere(4., [0.; 3]));
        assert!(s.max.iter().all(|v| *v <= 4. + 1e-9) && s.min.iter().all(|v| *v >= -4. - 1e-9));
    }
    #[test]
    fn ascii_stl_is_closed_and_bottom_points_down() {
        let stl = to_ascii_stl("part", &box_mesh([1.; 3], [0.; 3]));
        assert!(stl.starts_with("solid part\n"));
        assert!(stl.ends_with("endsolid part\n"));
        assert_eq!(stl.matches("facet normal").count(), 12);
        assert!(stl.contains("facet normal 0 0 -1"));
        assert!(!stl.contains(" -0"));
    }
    #[test]
    fn openscad_mirrors_parts() {
        let scad = to_scad(&[
            Part::Box {
                size: [1., 2., 3.],
                at: [4., 5., 6.],
            },
            Part::Cylinder {
                radius: 2.,
                height: 5.,
                at: [0.; 3],
            },
        ]);
        assert!(scad.contains("translate([4, 5, 6]) cube([1, 2, 3]);"));
        assert!(scad.contains("cylinder(r = 2, h = 5"));
    }
}
