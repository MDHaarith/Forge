//! The saved PCB copper model and feed rules shared by simulation and reader checks.
use crate::jsfmt::number;
use serde_json::Value;
use std::collections::BTreeMap;

fn g6(v: f64) -> String {
    if v == 0. {
        return if v.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let raw = format!("{v:.5e}");
    let (mantissa, exp) = raw.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if !(-4..6).contains(&exp) {
        format!(
            "{}e{exp:+03}",
            mantissa.trim_end_matches('0').trim_end_matches('.')
        )
    } else {
        number(raw.parse::<f64>().unwrap())
    }
}

pub type Point = [f64; 2];
#[derive(Clone)]
pub struct Sheet {
    pub z: f64,
    pub outer: Vec<Point>,
    pub holes: Vec<Vec<Point>>,
}
#[derive(Clone)]
pub struct Barrel {
    pub xy: Point,
    pub radius: f64,
    pub z: [f64; 2],
}
#[derive(Clone)]
pub struct Feed {
    pub start: [f64; 3],
    pub stop: [f64; 3],
    pub axis: usize,
    pub sign: f64,
    pub note: String,
}
pub struct Board {
    pub name: String,
    pub outline: Vec<Point>,
    pub thickness: f64,
    pub layers: Vec<f64>,
    pub copper: Vec<Sheet>,
    pub pours: Vec<Sheet>,
    pub barrels: Vec<Barrel>,
    pub feed: Feed,
    pub ground_under: f64,
}

#[cfg(test)]
mod tests {
    #[test]
    fn report_coordinates_keep_the_runner_six_digit_format() {
        for (v, s) in [
            (1.6 - 1.3896000000000002, "0.2104"),
            (1.23456789, "1.23457"),
            (1e-5, "1e-05"),
            (-0., "-0"),
        ] {
            assert_eq!(super::g6(v), s);
        }
    }
}

fn n(v: &Value, key: &str) -> Result<f64, String> {
    v[key]
        .as_f64()
        .filter(|x| x.is_finite() && x.abs() <= 100_000.)
        .ok_or_else(|| format!("invalid antenna geometry: {key} must be a finite number"))
}
fn positive(v: &Value, key: &str) -> Result<f64, String> {
    let x = n(v, key)?;
    if x <= 0. {
        return Err(format!("invalid antenna geometry: {key} must be positive"));
    }
    Ok(x)
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn xy(v: &Value) -> Result<Point, String> {
    Ok([n(v, "x")?, n(v, "y")?])
}
fn ring(v: &Value) -> Result<Vec<Point>, String> {
    let p = v.as_array().ok_or("invalid antenna polygon")?;
    if p.len() < 3 || p.len() > 100_000 {
        return Err("invalid antenna polygon".into());
    }
    p.iter().map(xy).collect()
}
pub fn rect(c: Point, w: f64, h: f64, angle: f64) -> Vec<Point> {
    let (s, cosa) = angle.to_radians().sin_cos();
    [
        (-w / 2., -h / 2.),
        (w / 2., -h / 2.),
        (w / 2., h / 2.),
        (-w / 2., h / 2.),
    ]
    .iter()
    .map(|&(x, y)| [c[0] + x * cosa - y * s, c[1] + x * s + y * cosa])
    .collect()
}
fn ngon(c: Point, r: f64, count: usize) -> Vec<Point> {
    (0..count)
        .map(|i| {
            let a = std::f64::consts::TAU * (i as f64 + 0.5) / count as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}
pub fn inside(poly: &[Point], p: Point) -> bool {
    let mut hit = false;
    for (a, b) in poly
        .iter()
        .zip(poly.iter().cycle().skip(1))
        .take(poly.len())
    {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        {
            hit = !hit;
        }
    }
    hit
}
impl Sheet {
    pub fn contains(&self, p: Point) -> bool {
        inside(&self.outer, p) && !self.holes.iter().any(|h| inside(h, p))
    }
}
fn pad_shape(v: &Value) -> Result<Vec<Point>, String> {
    // shortcut: preserve the runner's rectangular pill pads and 16-sided circles, use conformal edges when curved-pad accuracy is needed.
    Ok(match v["shape"].as_str().unwrap_or("rect") {
        "rect" | "rotated_rect" | "pill" | "rotated_pill" => rect(
            xy(v)?,
            positive(v, "width")?,
            positive(v, "height")?,
            v.get("ccw_rotation")
                .or_else(|| v.get("rotation"))
                .and_then(Value::as_f64)
                .unwrap_or(0.),
        ),
        "circle" => ngon(xy(v)?, positive(v, "radius")?, 16),
        "polygon" => ring(&v["points"])?,
        shape => return Err(format!("unsupported pad shape {shape}")),
    })
}

pub fn load(circuit: &Value, antenna: &str) -> Result<Board, String> {
    // shortcut: geometry is bounded to 100k elements/vertices and 1M trace samples, stream larger saved boards when needed.
    let rows = circuit
        .as_array()
        .ok_or("invalid antenna circuit: expected an array")?;
    if rows.len() > 100_000 {
        return Err("antenna circuit exceeds 100000 elements".into());
    }
    let board = rows
        .iter()
        .find(|e| text(e, "type") == "pcb_board")
        .ok_or("the circuit has no board")?;
    let thickness = board
        .get("thickness")
        .map_or(Ok(1.6), |_| positive(board, "thickness"))?;
    let mut z = BTreeMap::from([("bottom", 0.), ("top", thickness)]);
    if board["num_layers"].as_u64().unwrap_or(2) >= 4 {
        let d = if thickness >= 1. {
            0.2104
        } else {
            thickness / 8.
        };
        z.insert("inner1", thickness - d);
        z.insert("inner2", d);
    }
    let names: BTreeMap<_, _> = rows
        .iter()
        .filter(|e| text(e, "type") == "source_component")
        .map(|e| (text(e, "source_component_id"), text(e, "name")))
        .collect();
    let comps: BTreeMap<_, _> = rows
        .iter()
        .filter(|e| text(e, "type") == "pcb_component")
        .map(|e| {
            (
                text(e, "pcb_component_id"),
                *names.get(text(e, "source_component_id")).unwrap_or(&""),
            )
        })
        .collect();
    let drawn: std::collections::BTreeSet<_> = rows
        .iter()
        .filter(|e| text(e, "type") == "pcb_trace" && !text(e, "pcb_component_id").is_empty())
        .map(|e| text(e, "pcb_component_id"))
        .collect();
    let ant = if !antenna.is_empty() {
        comps
            .iter()
            .find(|(_, name)| **name == antenna)
            .map(|(id, _)| *id)
            .ok_or_else(|| format!("no component named {antenna}"))?
    } else if drawn.len() == 1 {
        *drawn.first().unwrap()
    } else if drawn.is_empty() {
        return Err("no antenna found; add an <antenna /> to the board".into());
    } else {
        return Err(format!(
            "several antennas on the board; name one: {}",
            drawn
                .iter()
                .map(|c| *comps.get(c).unwrap_or(&""))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };
    let name = comps.get(ant).unwrap_or(&"").to_string();
    let outline = if board["outline"].is_array() {
        ring(&board["outline"])?
    } else {
        let center = if board["center"].is_object() {
            xy(&board["center"])?
        } else {
            [0., 0.]
        };
        rect(
            center,
            positive(board, "width")?,
            positive(board, "height")?,
            0.,
        )
    };
    let mut copper = Vec::new();
    let mut arm = Vec::new();
    let mut pads = BTreeMap::new();
    let mut barrels = Vec::new();
    let mut pours = Vec::new();
    for e in rows {
        match text(e, "type") {
            "pcb_trace" if text(e, "pcb_component_id") == ant => {
                let route = e["route"].as_array().ok_or("invalid antenna trace route")?;
                let wires: Vec<_> = route
                    .iter()
                    .filter(|p| text(p, "route_type") == "wire")
                    .collect();
                for pair in wires.windows(2) {
                    let a = xy(pair[0])?;
                    let b = xy(pair[1])?;
                    let d = [b[0] - a[0], b[1] - a[1]];
                    let len = d[0].hypot(d[1]);
                    let count = (len / 0.25).floor().max(1.) as usize;
                    if arm.len() + count > 1_000_000 {
                        return Err("antenna traces exceed the geometry limit".into());
                    }
                    arm.extend((0..=count).map(|k| {
                        [
                            a[0] + d[0] * k as f64 / count as f64,
                            a[1] + d[1] * k as f64 / count as f64,
                        ]
                    }));
                    if len > 1e-9 && text(pair[0], "layer") == text(pair[1], "layer") {
                        let w = positive(pair[0], "width")? / 2.;
                        let normal = [-d[1] / len * w, d[0] / len * w];
                        if let Some(&height) = z.get(text(pair[0], "layer")) {
                            copper.push(Sheet {
                                z: height,
                                holes: vec![],
                                outer: vec![
                                    [a[0] + normal[0], a[1] + normal[1]],
                                    [b[0] + normal[0], b[1] + normal[1]],
                                    [b[0] - normal[0], b[1] - normal[1]],
                                    [a[0] - normal[0], a[1] - normal[1]],
                                ],
                            });
                        }
                    }
                }
                for (i, p) in wires.iter().enumerate() {
                    let c = xy(p)?;
                    let w = positive(p, "width")?;
                    let square = wires
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| j.abs_diff(i) == 1)
                        .all(|(_, q)| {
                            (c[0] - q["x"].as_f64().unwrap_or(f64::NAN)).abs() < 1e-9
                                || (c[1] - q["y"].as_f64().unwrap_or(f64::NAN)).abs() < 1e-9
                        });
                    if let Some(&height) = z.get(text(p, "layer")) {
                        copper.push(Sheet {
                            z: height,
                            holes: vec![],
                            outer: if square {
                                rect(c, w, w, 0.)
                            } else {
                                ngon(c, w / 2., 8)
                            },
                        });
                    }
                }
            }
            "pcb_smtpad" if text(e, "pcb_component_id") == ant => {
                let height = *z.get(text(e, "layer")).ok_or("invalid antenna pad layer")?;
                copper.push(Sheet {
                    z: height,
                    outer: pad_shape(e)?,
                    holes: vec![],
                });
                if let Some(hints) = e["port_hints"].as_array() {
                    for hint in hints {
                        if let Some(h) = hint.as_str() {
                            pads.insert(h, (false, e));
                        }
                    }
                }
            }
            "pcb_via" | "pcb_plated_hole" => {
                if text(e, "type") == "pcb_plated_hole" && text(e, "pcb_component_id") != ant {
                    continue;
                }
                let c = xy(e)?;
                let outer = e
                    .get("outer_diameter")
                    .or_else(|| e.get("outer_width"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.6);
                let hole = e
                    .get("hole_diameter")
                    .or_else(|| e.get("hole_width"))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.3);
                if !outer.is_finite()
                    || !hole.is_finite()
                    || outer <= 0.
                    || hole <= 0.
                    || outer > 100_000.
                    || hole > outer
                {
                    return Err("invalid antenna via dimensions".into());
                }
                let layers: Vec<_> =
                    if let Some(ls) = e["layers"].as_array().filter(|ls| !ls.is_empty()) {
                        ls.iter().filter_map(Value::as_str).collect()
                    } else {
                        vec![
                            e["from_layer"].as_str().unwrap_or("top"),
                            e["to_layer"].as_str().unwrap_or("bottom"),
                        ]
                    };
                let mut heights = Vec::new();
                for layer in layers {
                    if let Some(&height) = z.get(layer) {
                        heights.push(height);
                        copper.push(Sheet {
                            z: height,
                            outer: ngon(c, outer / 2., 16),
                            holes: vec![],
                        });
                    }
                }
                if heights.len() >= 2 {
                    barrels.push(Barrel {
                        xy: c,
                        radius: hole / 2.,
                        z: [
                            heights.iter().copied().fold(f64::INFINITY, f64::min),
                            heights.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                        ],
                    });
                }
                if text(e, "type") == "pcb_plated_hole" {
                    if let Some(hints) = e["port_hints"].as_array() {
                        for hint in hints {
                            if let Some(h) = hint.as_str() {
                                pads.insert(h, (true, e));
                            }
                        }
                    }
                }
            }
            "pcb_copper_pour" => {
                let Some(&height) = z.get(text(e, "layer")) else {
                    continue;
                };
                let (outer, holes) = match text(e, "shape") {
                    "brep" => (
                        ring(&e["brep_shape"]["outer_ring"]["vertices"])?,
                        e["brep_shape"]["inner_rings"]
                            .as_array()
                            .map_or(Ok(vec![]), |rings| {
                                rings
                                    .iter()
                                    .map(|r| ring(&r["vertices"]))
                                    .collect::<Result<Vec<_>, _>>()
                            })?,
                    ),
                    "rect" => (
                        rect(
                            xy(&e["center"])?,
                            positive(e, "width")?,
                            positive(e, "height")?,
                            e["rotation"].as_f64().unwrap_or(0.),
                        ),
                        vec![],
                    ),
                    "polygon" => (ring(&e["points"])?, vec![]),
                    shape => return Err(format!("unsupported copper pour shape {shape}")),
                };
                pours.push(Sheet {
                    z: height,
                    outer,
                    holes,
                });
            }
            _ => (),
        }
    }
    let &(hole, p1) = pads
        .get("pin1")
        .ok_or_else(|| format!("antenna {name} has no pin1 feed pad"))?;
    let p = xy(p1)?;
    let zp = *z
        .get(if hole { "top" } else { text(p1, "layer") })
        .ok_or("invalid antenna pad layer")?;
    let feed = if let Some(&(false, p2)) = pads
        .get("pin2")
        .filter(|(_, v)| text(v, "layer") == if hole { "top" } else { text(p1, "layer") })
    {
        let q = xy(p2)?;
        let axis = if (q[1] - p[1]).abs() < 1e-6 {
            0
        } else if (q[0] - p[0]).abs() < 1e-6 {
            1
        } else {
            return Err("pin1 and pin2 of the antenna must line up along x or y".into());
        };
        let key = if axis == 0 { "width" } else { "height" };
        let (low, high) = if p[axis] < q[axis] {
            (p1, p2)
        } else {
            (p2, p1)
        };
        let mut start = [p[0], p[1], zp];
        let mut stop = start;
        start[axis] = p[axis].min(q[axis]) + positive(low, key)? / 2.;
        stop[axis] = p[axis].max(q[axis]) - positive(high, key)? / 2.;
        Feed {
            start,
            stop,
            axis,
            sign: if q[axis] > p[axis] { 1. } else { -1. },
            note: "across the gap between pin1 and pin2".into(),
        }
    } else {
        let zg=pours.iter().filter(|s| s.z!=zp && s.contains(p)).min_by(|a,b| (a.z-zp).abs().total_cmp(&(b.z-zp).abs())).map(|s|s.z)
            .ok_or_else(||format!("no ground copper under the antenna feed at ({}, {}) mm; add <copperpour connectsTo=\"net.GND\" layer=\"bottom\" /> so the feed has a ground reference",g6(p[0]),g6(p[1])))?;
        Feed {
            start: [p[0], p[1], zg.min(zp)],
            stop: [p[0], p[1], zg.max(zp)],
            axis: 2,
            sign: if zp > zg { 1. } else { -1. },
            note: format!(
                "from the ground plane up to the pin1 pad ({} mm probe)",
                g6((zp - zg).abs())
            ),
        }
    };
    if feed.stop[feed.axis] - feed.start[feed.axis] <= 1e-9 {
        return Err("antenna feed pads overlap or have no gap".into());
    }
    let ends: Vec<_> = pads
        .values()
        .map(|(_, v)| xy(v))
        .collect::<Result<_, _>>()?;
    let far: Vec<_> = arm
        .iter()
        .filter(|p| ends.iter().all(|e| (p[0] - e[0]).hypot(p[1] - e[1]) > 3.))
        .collect();
    let covered = |p| pours.iter().any(|s| s.contains(p));
    let grounded = far
        .iter()
        .filter(|p| {
            covered(***p)
                || (0..8).any(|k| {
                    let a = k as f64 * std::f64::consts::FRAC_PI_4;
                    covered([p[0] + 2. * a.cos(), p[1] + 2. * a.sin()])
                })
        })
        .count();
    let ground_under = if far.is_empty() {
        0.
    } else {
        grounded as f64 / far.len() as f64
    };
    let mut layers: Vec<_> = z.values().copied().collect();
    layers.sort_by(f64::total_cmp);
    Ok(Board {
        name,
        outline,
        thickness,
        layers,
        copper,
        pours,
        barrels,
        feed,
        ground_under,
    })
}
