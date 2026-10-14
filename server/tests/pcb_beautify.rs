mod common;
#[path = "common/tools.rs"]
mod support;

use forge_server::tools::pcb;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn board() -> Value {
    json!({"board":{"width":40,"height":30},"parts":[
        {"ref":"U1","kind":"chip","footprint":"soic8","at":[0,0]},
        {"ref":"R1","kind":"resistor","footprint":"0603","at":[-10,5]},
        {"ref":"R2","kind":"resistor","footprint":"0603","at":[-10,-5]},
        {"ref":"R3","kind":"resistor","footprint":"0603","at":[10,5]},
        {"ref":"C1","kind":"capacitor","footprint":"0603","at":[-10,-10]},
        {"ref":"C2","kind":"capacitor","footprint":"0603","at":[10,-5]},
        {"ref":"LED1","kind":"led","footprint":"0603","at":[15,5]},
        {"ref":"J1","kind":"header","footprint":"pinheader_1x2","at":[0,-10]}],
        "nets":{"VCC":["J1.1","U1.8","U1.4","R1.1"],
        "GND":["J1.2","U1.1","C1.2","C2.2","LED1.K"],
        "DISCH":["R1.2","R2.1","U1.7"],"TIMING":["R2.2","C1.1","U1.6","U1.2"],
        "CTRL":["U1.5","C2.1"],"OUT":["U1.3","R3.1"],"LED_NET":["R3.2","LED1.A"]}})
}

fn build(spec: &Value) -> pcb::NativeBuild {
    pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap()
}
fn elements(b: &pcb::NativeBuild) -> Vec<Value> {
    serde_json::from_str::<Value>(&b.files[0].1)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}
fn point(v: &Value) -> [f64; 2] {
    [v["x"].as_f64().unwrap(), v["y"].as_f64().unwrap()]
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn count(es: &[Value], kind: &str) -> usize {
    es.iter().filter(|e| e["type"] == kind).count()
}
fn lengths(es: &[Value]) -> BTreeMap<String, f64> {
    let mut result = BTreeMap::new();
    for t in es.iter().filter(|e| e["type"] == "pcb_trace") {
        *result
            .entry(t["net"].as_str().unwrap().to_owned())
            .or_default() += distance(point(&t["route"][0]), point(&t["route"][1]));
    }
    result
}
async fn drc(spec: &Value, es: &[Value]) -> String {
    let dir = support::project(
        "pcb-beautify",
        &[
            ("pcb/test.pcb.json", spec.to_string()),
            ("pcb/test.circuit.json", serde_json::to_string(es).unwrap()),
        ],
    );
    support::run("pcb_drc", json!({"board":"test"}), &dir.path)
        .await
        .unwrap()
        .text
}

fn segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0] * d[0] + d[1] * d[1];
    let t = if l == 0. {
        0.
    } else {
        ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l
    }
    .clamp(0., 1.);
    distance(p, [a[0] + t * d[0], a[1] + t * d[1]])
}
fn segment_gap(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> f64 {
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    if cross(a, b, c) * cross(a, b, d) < 0. && cross(c, d, a) * cross(c, d, b) < 0. {
        0.
    } else {
        segment_distance(a, c, d)
            .min(segment_distance(b, c, d))
            .min(segment_distance(c, a, b))
            .min(segment_distance(d, a, b))
    }
}
fn pad_contains(p: &Value, at: [f64; 2], margin: f64) -> bool {
    let center = point(p);
    if p["type"] == "pcb_plated_hole" {
        return distance(center, at) <= p["outer_diameter"].as_f64().unwrap() / 2. + margin;
    }
    let (s, c) = p["rotation"].as_f64().unwrap_or(0.).to_radians().sin_cos();
    let d = [at[0] - center[0], at[1] - center[1]];
    (c * d[0] + s * d[1]).abs() <= p["width"].as_f64().unwrap() / 2. + margin
        && (-s * d[0] + c * d[1]).abs() <= p["height"].as_f64().unwrap() / 2. + margin
}

/// An independent union-find over saved pads, copper segments and physical via spans.
fn assert_connected(es: &[Value]) {
    let mut nets: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for e in es.iter().filter(|e| {
        matches!(
            e["type"].as_str(),
            Some("pcb_smtpad" | "pcb_plated_hole" | "pcb_trace" | "pcb_via")
        )
    }) {
        if let Some(n) = e["net"].as_str() {
            nets.entry(n).or_default().push(e);
        }
    }
    for (net, items) in nets {
        let mut parents: Vec<_> = (0..items.len()).collect();
        let root = |parents: &[usize], mut i: usize| {
            while parents[i] != i {
                i = parents[i];
            }
            i
        };
        let layers = |e: &Value| -> Vec<String> {
            if e["type"] == "pcb_trace" {
                vec![e["route"][0]["layer"].as_str().unwrap().into()]
            } else if let Some(ls) = e["layers"].as_array() {
                ls.iter().map(|l| l.as_str().unwrap().into()).collect()
            } else {
                vec![e["layer"].as_str().unwrap().into()]
            }
        };
        let stroke = |e: &Value| {
            if e["type"] == "pcb_trace" {
                (
                    point(&e["route"][0]),
                    point(&e["route"][1]),
                    e["width"].as_f64().unwrap() / 2.,
                )
            } else {
                (
                    point(e),
                    point(e),
                    e["outer_diameter"].as_f64().unwrap() / 2.,
                )
            }
        };
        let is_pad = |e: &Value| e["type"] == "pcb_smtpad" || e["type"] == "pcb_plated_hole";
        for (i, a) in items.iter().enumerate() {
            for (j, b) in items[..i].iter().enumerate() {
                if !layers(a).iter().any(|l| layers(b).contains(l)) {
                    continue;
                }
                let touch = if is_pad(a) && is_pad(b) {
                    pad_contains(a, point(b), 0.) || pad_contains(b, point(a), 0.)
                } else if is_pad(a) || is_pad(b) {
                    let (p, t) = if is_pad(a) { (a, b) } else { (b, a) };
                    let (u, v, r) = stroke(t);
                    pad_contains(p, u, r)
                        || pad_contains(p, v, r)
                        || segment_distance(point(p), u, v) <= r + 1e-6
                } else {
                    let (u, v, r) = stroke(a);
                    let (x, y, s) = stroke(b);
                    segment_gap(u, v, x, y) <= r + s + 1e-6
                };
                if touch {
                    let x = root(&parents, i);
                    let y = root(&parents, j);
                    parents[x] = y;
                }
            }
        }
        let pads: Vec<_> = items
            .iter()
            .enumerate()
            .filter(|(_, e)| is_pad(e))
            .map(|(i, _)| i)
            .collect();
        if let Some(&first) = pads.first() {
            assert!(
                pads.iter()
                    .all(|&i| root(&parents, i) == root(&parents, first)),
                "disconnected saved pads: {net}"
            );
        }
    }
}

async fn assert_no_clear_right_corner(spec: &Value, es: &[Value]) {
    let tracks: Vec<_> = es
        .iter()
        .enumerate()
        .filter(|(_, e)| e["type"] == "pcb_trace")
        .collect();
    for (k, (i, a)) in tracks.iter().enumerate() {
        for (j, b) in &tracks[..k] {
            if a["net"] != b["net"] || a["route"][0]["layer"] != b["route"][0]["layer"] {
                continue;
            }
            for (ai, bi) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                let c = point(&a["route"][ai]);
                if distance(c, point(&b["route"][bi])) > 1e-6 {
                    continue;
                }
                let p = point(&a["route"][1 - ai]);
                let q = point(&b["route"][1 - bi]);
                let (u, v) = (distance(p, c), distance(q, c));
                if u < 0.5
                    || v < 0.5
                    || ((p[0] - c[0]) * (q[0] - c[0]) + (p[1] - c[1]) * (q[1] - c[1])).abs() > 1e-5
                {
                    continue;
                }
                let degree = tracks
                    .iter()
                    .filter(|(_, t)| {
                        t["net"] == a["net"]
                            && t["route"][0]["layer"] == a["route"][0]["layer"]
                            && (distance(c, point(&t["route"][0])) < 1e-6
                                || distance(c, point(&t["route"][1])) < 1e-6)
                    })
                    .count();
                if degree != 2
                    || es.iter().any(|e| {
                        e["net"] == a["net"]
                            && ((e["type"] == "pcb_via" && distance(point(e), c) < 1e-6)
                                || (matches!(
                                    e["type"].as_str(),
                                    Some("pcb_smtpad" | "pcb_plated_hole")
                                ) && pad_contains(e, c, 0.)))
                    })
                {
                    continue;
                }
                let cut = 1_f64
                    .min(3. * a["width"].as_f64().unwrap())
                    .min(0.45 * u.min(v));
                let x = [
                    c[0] + (p[0] - c[0]) * cut / u,
                    c[1] + (p[1] - c[1]) * cut / u,
                ];
                let y = [
                    c[0] + (q[0] - c[0]) * cut / v,
                    c[1] + (q[1] - c[1]) * cut / v,
                ];
                let mut candidate = es.to_vec();
                candidate[*i]["route"][ai]["x"] = json!(x[0]);
                candidate[*i]["route"][ai]["y"] = json!(x[1]);
                candidate[*j]["route"][bi]["x"] = json!(y[0]);
                candidate[*j]["route"][bi]["y"] = json!(y[1]);
                let mut diagonal = (*a).clone();
                diagonal["pcb_trace_id"] = json!("corner_probe");
                diagonal["route"][0]["x"] = json!(x[0]);
                diagonal["route"][0]["y"] = json!(x[1]);
                diagonal["route"][1]["x"] = json!(y[0]);
                diagonal["route"][1]["y"] = json!(y[1]);
                candidate.push(diagonal);
                let report = drc(spec, &candidate).await;
                assert!(
                    !report.ends_with("No DRC errors."),
                    "clear 90-degree corner remained at {c:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn small_board_is_shorter_octilinear_connected_and_drc_clean() {
    let mut spec = board();
    spec["rules"] = json!({"beautify":false});
    let raw = build(&spec);
    let off = elements(&raw);
    spec["rules"]["beautify"] = json!(true);
    let built = build(&spec);
    let on = elements(&built);
    let before = count(&off, "pcb_trace");
    let after = count(&on, "pcb_trace");
    eprintln!(
        "Small board: segments {before} -> {after}, vias {} -> {}, length {:.6} -> {:.6} mm\n{}",
        count(&off, "pcb_via"),
        count(&on, "pcb_via"),
        lengths(&off).values().sum::<f64>(),
        lengths(&on).values().sum::<f64>(),
        built.report
    );
    assert!(
        after * 4 <= before * 3,
        "less than 25% fewer segments: {before} -> {after}"
    );
    assert!(count(&on, "pcb_via") <= count(&off, "pcb_via"));
    for (net, len) in lengths(&on) {
        assert!(len <= lengths(&off)[&net] + 0.200001, "{net}: length grew");
    }
    assert_connected(&off);
    assert_connected(&on);
    for t in on.iter().filter(|e| e["type"] == "pcb_trace") {
        let (a, b) = (point(&t["route"][0]), point(&t["route"][1]));
        let (dx, dy) = ((a[0] - b[0]).abs(), (a[1] - b[1]).abs());
        let oct = dx < 2e-6 || dy < 2e-6 || (dx - dy).abs() < 2e-6;
        assert!(
            oct || (distance(a, b) <= 0.600001
                && on.iter().any(|p| p["type"] == "pcb_smtpad"
                    && p["net"] == t["net"]
                    && (distance(point(p), a) < 1e-6 || distance(point(p), b) < 1e-6))),
            "non-octilinear trace: {t}"
        );
    }
    let report = drc(&spec, &on).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
    assert_no_clear_right_corner(&spec, &on).await;
    let report = drc(&spec, &off).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
    assert!(built
        .report
        .lines()
        .nth(3)
        .unwrap()
        .starts_with("Beautified:"));
    let again = build(&spec);
    assert_eq!(built.files, again.files);
    assert_eq!(built.binary_files, again.binary_files);
    assert_eq!(built.report, again.report);
}

#[tokio::test]
async fn reference_planes_keep_vias_and_never_gain_tracks() {
    let mut spec = board();
    spec["board"]["layers"] = json!(4);
    spec["pours"] = json!([{"net":"GND","layer":"inner1"}]);
    spec["rules"] = json!({"beautify":false});
    let off = elements(&build(&spec));
    spec["rules"]["beautify"] = json!(true);
    let on = elements(&build(&spec));
    assert!(on
        .iter()
        .filter(|e| e["type"] == "pcb_trace")
        .all(|t| t["route"][0]["layer"] != "inner1"));
    let plane_vias = |es: &[Value]| {
        es.iter()
            .filter(|e| e["type"] == "pcb_via" && e["net"] == "GND")
            .count()
    };
    assert_eq!(plane_vias(&off), plane_vias(&on));
    let report = drc(&spec, &on).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[tokio::test]
async fn coupled_pair_and_length_group_still_meet_constraints() {
    let mut spec = json!({"board":{"width":50,"height":40,"layers":4},"rules":{"clearance":0.15,"beautify":false},"parts":[
        {"ref":"J1","kind":"header","footprint":"usb_micro_b","at":[-14,-10],"rotation":-90},
        {"ref":"U1","kind":"chip","footprint":"soic8","at":[14,-10]},
        {"ref":"A0","kind":"resistor","footprint":"0603","at":[-16,4]},
        {"ref":"B0","kind":"resistor","footprint":"0603","at":[8,4]},
        {"ref":"A1","kind":"resistor","footprint":"0603","at":[-16,12]},
        {"ref":"B1","kind":"resistor","footprint":"0603","at":[12,12]}],
        "nets":{"DP":["J1.3","U1.2"],"DN":["J1.2","U1.1"],"D0":["A0.2","B0.1"],"D1":["A1.2","B1.1"]},
        "diffpairs":{"USB":{"p":"DP","n":"DN","gap":0.15,"width":0.2}},
        "length_groups":{"DATA":{"nets":["D0","D1"],"tolerance":0.5}}});
    let off = build(&spec);
    spec["rules"]["beautify"] = json!(true);
    let on = build(&spec);
    let pair = |es: Vec<Value>| {
        es.into_iter()
            .filter(|e| {
                (e["type"] == "pcb_trace" || e["type"] == "pcb_via")
                    && (e["net"] == "DP" || e["net"] == "DN")
            })
            .map(|mut e| {
                e.as_object_mut().unwrap().remove("pcb_trace_id");
                e
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(pair(elements(&off)), pair(elements(&on)));
    for b in [&off, &on] {
        let es = elements(b);
        let lens = lengths(&es);
        assert!((lens["DP"] - lens["DN"]).abs() <= 0.100001);
        assert!((lens["D0"] - lens["D1"]).abs() <= 0.500001);
        let line = b
            .report
            .lines()
            .find(|l| l.starts_with("Diffpair USB:"))
            .unwrap();
        let percent = line
            .split("coupled ")
            .nth(1)
            .unwrap()
            .split('%')
            .next()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!(percent >= 80., "{line}");
        assert_connected(&es);
        let report = drc(&spec, &es).await;
        assert!(report.ends_with("No DRC errors."), "{report}");
    }
}

#[test]
fn switch_is_default_true_and_strict() {
    assert!(pcb::parse(&board().to_string()).unwrap().rules.beautify);
    for bad in [json!(1), json!("false"), Value::Null] {
        let mut spec = board();
        spec["rules"] = json!({"beautify":bad});
        assert!(pcb::parse(&spec.to_string()).is_err());
    }
    let spec = json!({"board":{"width":10,"height":10}});
    assert!(!build(&spec).report.contains("Beautified:"));
}
