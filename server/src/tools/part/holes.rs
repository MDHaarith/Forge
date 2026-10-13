use super::{ops, standards::*, DVec3, Solid};
use crate::jsfmt::{fixed, json_number};
use cadrum::{Mesh, SurfaceKind};
use serde_json::{json, Value};
use std::{collections::HashMap, f64::consts::TAU};

// shortcut: threaded holes are not reported by part_inspect, add helical-face detection when users ask.
pub(super) fn describe_diameter(d: f64, through: bool) -> Option<String> {
    let tap = || {
        TAP_DRILL
            .iter()
            .find(|(_, drill)| (drill - d).abs() < 0.02)
            .map(|(size, _)| format!("{size} tap"))
    };
    let insert = || {
        INSERT
            .iter()
            .find(|(_, data)| (data[0] - d).abs() < 0.02)
            .map(|(size, _)| format!("{size} insert"))
    };
    let clear = || {
        for (size, diameters) in CLEARANCE {
            for (fit, i) in FITS {
                if (diameters[*i] - d).abs() < 0.02 {
                    return Some(format!(
                        "{size} clearance{}",
                        if *fit == "normal" {
                            String::new()
                        } else {
                            format!(" ({fit})")
                        }
                    ));
                }
            }
        }
        None
    };
    tap().or_else(|| {
        if through {
            clear().or_else(insert)
        } else {
            insert().or_else(clear)
        }
    })
}

struct Cylinder {
    diameter: f64,
    lo: f64,
    hi: f64,
    sweep: f64,
}

struct Cone {
    hi: f64,
    circles: Vec<f64>,
}

struct Group {
    axis: DVec3,
    across: DVec3,
    cylinders: Vec<Cylinder>,
    cones: Vec<Cone>,
}

pub(super) fn holes_in(solids: &[Solid], mesh: &Mesh) -> Vec<Value> {
    let tol = 1e-3;
    let mut face_points = HashMap::new();
    for (triangle, id) in mesh.indices.chunks_exact(3).zip(&mesh.face_ids) {
        face_points.entry(*id).or_insert(mesh.vertices[triangle[0]]);
    }
    let mut groups: Vec<([i64; 6], Group)> = Vec::new();
    for face in solids.iter().flat_map(Solid::iter_face) {
        let Some(surf) = face.surface() else { continue };
        if !matches!(
            surf.kind,
            SurfaceKind::Cylinder { .. } | SurfaceKind::Cone { .. }
        ) {
            continue;
        }
        let mut axis = surf.axis_z.normalize();
        if axis.z < -tol
            || (axis.z.abs() < tol && (axis.y < -tol || (axis.y.abs() < tol && axis.x < 0.0)))
        {
            axis = -axis;
        }
        let origin = match surf.kind {
            SurfaceKind::Cone { radius, semi_angle } => {
                surf.origin - surf.axis_z * (radius / semi_angle.tan())
            }
            _ => surf.origin,
        };
        let Some(point) = face_points.get(&face.id()) else {
            continue;
        };
        let (point, normal) = face.project(*point);
        let foot = origin + axis * (point - origin).dot(axis);
        let radial = point - foot;
        if radial.length() < tol || normal.dot(radial) > 0.0 {
            continue;
        }
        let across = origin - axis * origin.dot(axis);
        let key = [
            (axis.x * 1000.0).round_ties_even() as i64,
            (axis.y * 1000.0).round_ties_even() as i64,
            (axis.z * 1000.0).round_ties_even() as i64,
            (across.x * 100.0).round_ties_even() as i64,
            (across.y * 100.0).round_ties_even() as i64,
            (across.z * 100.0).round_ties_even() as i64,
        ];
        let edges: Vec<_> = face.iter_edge().map(ops::samples).collect();
        let bbox = edges
            .iter()
            .flatten()
            .map(|p| [*p, *p])
            .reduce(|[min, max], [a, b]| [min.min(a), max.max(b)])
            .unwrap_or([origin; 2]);
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for x in [bbox[0].x, bbox[1].x] {
            for y in [bbox[0].y, bbox[1].y] {
                for z in [bbox[0].z, bbox[1].z] {
                    let t = DVec3::new(x, y, z).dot(axis);
                    lo = lo.min(t);
                    hi = hi.max(t);
                }
            }
        }
        let group_index = groups
            .iter()
            .position(|(k, _)| *k == key)
            .unwrap_or_else(|| {
                groups.push((
                    key,
                    Group {
                        axis,
                        across,
                        cylinders: vec![],
                        cones: vec![],
                    },
                ));
                groups.len() - 1
            });
        let g = &mut groups[group_index].1;
        match surf.kind {
            SurfaceKind::Cylinder { radius } => {
                // The public API has no UV range; boundary winding gives the cylinder's angular span.
                let sweep = edges
                    .iter()
                    .map(|points| {
                        points
                            .windows(2)
                            .map(|p| {
                                let a = p[0] - origin - axis * (p[0] - origin).dot(axis);
                                let b = p[1] - origin - axis * (p[1] - origin).dot(axis);
                                axis.dot(a.cross(b)).atan2(a.dot(b)).abs()
                            })
                            .sum::<f64>()
                    })
                    .fold(0.0, f64::max);
                g.cylinders.push(Cylinder {
                    diameter: radius * 2.0,
                    lo,
                    hi,
                    sweep,
                });
            }
            SurfaceKind::Cone { .. } => {
                let circles = edges
                    .iter()
                    .filter_map(|points| {
                        let p = *points.first()?;
                        let axial = (p - origin).dot(axis);
                        let r = (p - origin - axis * axial).length();
                        if points.len() > 2
                            && r > tol
                            && points.iter().all(|p| {
                                let t = (*p - origin).dot(axis);
                                (t - axial).abs() < 1e-6
                                    && ((*p - origin - axis * t).length() - r).abs() < 1e-6
                            })
                        {
                            Some(r * 2.0)
                        } else {
                            None
                        }
                    })
                    .collect();
                g.cones.push(Cone { hi, circles });
            }
            _ => unreachable!(),
        }
    }
    let mut found = Vec::new();
    for (_, mut g) in groups {
        if g.cylinders.is_empty() {
            continue;
        }
        g.cylinders
            .sort_by(|a, b| a.diameter.total_cmp(&b.diameter));
        let smallest = g.cylinders[0].diameter;
        if g.cylinders
            .iter()
            .filter(|c| (c.diameter - smallest).abs() < tol)
            .map(|c| c.sweep)
            .sum::<f64>()
            < TAU - 1e-3
        {
            continue;
        }
        let diameter = fixed(smallest, 3);
        let hi = g
            .cylinders
            .iter()
            .map(|c| c.hi)
            .chain(g.cones.iter().map(|c| c.hi))
            .fold(f64::NEG_INFINITY, f64::max);
        let bore_lo = g
            .cylinders
            .iter()
            .filter(|c| (c.diameter - smallest).abs() < tol)
            .map(|c| c.lo)
            .fold(f64::INFINITY, f64::min);
        let open_top = !solids
            .iter()
            .any(|s| s.contains(g.across + g.axis * (hi + 0.05)));
        let open_bottom = !solids
            .iter()
            .any(|s| s.contains(g.across + g.axis * (bore_lo - 0.05)));
        let through = open_top && open_bottom;
        let entry = g.across
            + g.axis
                * if open_top || !open_bottom {
                    hi
                } else {
                    bore_lo
                };
        let axis = if g.axis.z.abs() > 1.0 - tol {
            "Z"
        } else if g.axis.y.abs() > 1.0 - tol {
            "Y"
        } else if g.axis.x.abs() > 1.0 - tol {
            "X"
        } else {
            "other"
        };
        let mut hole = json!({"axis":axis,"diameter":json_number(diameter),"through":through,
            "depth":json_number(fixed(hi-bore_lo,3)),"center":entry.to_array().map(|n| json_number(fixed(n,3)))});
        if let Some(c) = g
            .cylinders
            .iter()
            .rev()
            .find(|c| c.diameter > smallest + tol)
        {
            hole["counterbore"] = json!([
                json_number(fixed(c.diameter, 3)),
                json_number(fixed(hi - c.lo, 3))
            ]);
        }
        for c in g.cones {
            if c.circles.is_empty() {
                continue;
            }
            let small = c.circles.iter().copied().fold(f64::INFINITY, f64::min);
            let big = c.circles.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if (small - diameter).abs() < 0.05 && (big - small) / 2.0 >= 0.25 * diameter {
                hole["countersink"] = json!(json_number(fixed(big, 3)));
            }
        }
        hole["fits"] = json!(describe_diameter(diameter, through));
        found.push(hole);
    }
    found.sort_by(|a, b| {
        a["axis"]
            .as_str()
            .cmp(&b["axis"].as_str())
            .then_with(|| {
                b["center"][2]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&a["center"][2].as_f64().unwrap())
            })
            .then_with(|| {
                a["center"][1]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&b["center"][1].as_f64().unwrap())
            })
            .then_with(|| {
                a["center"][0]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&b["center"][0].as_f64().unwrap())
            })
    });
    found
}
