#![allow(unused)]

mod common;
#[path = "common/tools.rs"]
mod support;

use forge_server::tools::pcb::{self, NativeBuild};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn medium_board_spec() -> Value {
    json!({
        "board": {"width": 50, "height": 40, "layers": 2},
        "rules": {"clearance": 0.15},
        "parts": [
            {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [-12, 8]},
            {"ref": "U2", "kind": "chip", "footprint": "soic16", "at": [12, 8]},
            {"ref": "U3", "kind": "chip", "footprint": "qfn16", "at": [0, -6]},
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x4", "at": [-18, -10]},
            {"ref": "J2", "kind": "header", "footprint": "pinheader_1x4", "at": [18, -10]},
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [-14, -2]},
            {"ref": "R2", "kind": "resistor", "footprint": "0603", "at": [-5, 8]},
            {"ref": "R3", "kind": "resistor", "footprint": "0603", "at": [5, 8]},
            {"ref": "R4", "kind": "resistor", "footprint": "0603", "at": [14, -2]},
            {"ref": "C1", "kind": "capacitor", "footprint": "0402", "at": [-12, 16]},
            {"ref": "C2", "kind": "capacitor", "footprint": "0402", "at": [-4, 16]},
            {"ref": "C3", "kind": "capacitor", "footprint": "0402", "at": [4, 16]},
            {"ref": "C4", "kind": "capacitor", "footprint": "0402", "at": [12, 16]},
            {"ref": "D1", "kind": "led", "footprint": "0603", "at": [0, 4]}
        ],
        "nets": {
            "N1": ["J1.1", "U1.1"],
            "N2": ["J1.2", "U1.2"],
            "N3": ["J1.3", "R1.1"],
            "N4": ["R1.2", "U1.3"],
            "N5": ["J1.4", "C1.1"],
            "N6": ["C1.2", "U1.4"],
            "N7": ["U1.5", "R2.1"],
            "N8": ["R2.2", "U3.1"],
            "N9": ["U1.6", "C2.1"],
            "N10": ["C2.2", "U3.2"],
            "N11": ["U1.7", "D1.1"],
            "N12": ["D1.2", "U3.3"],
            "N13": ["U1.8", "U2.1"],
            "N14": ["U3.4", "R3.1"],
            "N15": ["R3.2", "U2.2"],
            "N16": ["U3.5", "C3.1"],
            "N17": ["C3.2", "U2.3"],
            "N18": ["U3.6", "U2.4"],
            "N19": ["U2.5", "R4.1"],
            "N20": ["R4.2", "J2.1"],
            "N21": ["U2.6", "C4.1"],
            "N22": ["C4.2", "J2.2"],
            "N23": ["U2.7", "J2.3"],
            "N24": ["U2.8", "J2.4"]
        }
    })
}

fn build(spec: &Value) -> NativeBuild {
    pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap()
}

fn elements(b: &NativeBuild) -> Vec<Value> {
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

async fn drc(spec: &Value, built: &NativeBuild) -> String {
    let dir = support::project(
        "pcb-beautify-bb",
        &[
            ("pcb/test.pcb.json", spec.to_string()),
            ("pcb/test.circuit.json", built.files[0].1.clone()),
        ],
    );
    support::run("pcb_drc", json!({"board": "test"}), &dir.path)
        .await
        .unwrap()
        .text
}

fn assert_connected_from_circuit_json(elements: &[Value]) {
    let mut nets: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for e in elements {
        if let Some(net) = e.get("net").and_then(|n| n.as_str()) {
            let ty = e.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if matches!(ty, "pcb_smtpad" | "pcb_plated_hole" | "pcb_trace" | "pcb_via") {
                nets.entry(net).or_default().push(e);
            }
        }
    }

    for (net, items) in nets {
        let pads: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let ty = e["type"].as_str().unwrap_or("");
                ty == "pcb_smtpad" || ty == "pcb_plated_hole"
            })
            .map(|(i, _)| i)
            .collect();

        if pads.len() <= 1 {
            continue;
        }

        let mut parent: Vec<usize> = (0..items.len()).collect();
        fn find(parent: &mut [usize], mut i: usize) -> usize {
            let mut root = i;
            while parent[root] != root {
                root = parent[root];
            }
            while parent[i] != root {
                let nxt = parent[i];
                parent[i] = root;
                i = nxt;
            }
            root
        }
        fn union(parent: &mut [usize], a: usize, b: usize) {
            let ra = find(parent, a);
            let rb = find(parent, b);
            if ra != rb {
                parent[ra] = rb;
            }
        }

        let pad_contains = |pad: &Value, pt: [f64; 2]| -> bool {
            let cx = pad["x"].as_f64().unwrap();
            let cy = pad["y"].as_f64().unwrap();
            if pad["type"] == "pcb_plated_hole" {
                let r = pad["outer_diameter"].as_f64().unwrap() / 2.0;
                (pt[0] - cx).hypot(pt[1] - cy) <= r + 1e-4
            } else {
                let rot = pad["rotation"].as_f64().unwrap_or(0.0).to_radians();
                let (s, c) = rot.sin_cos();
                let dx = pt[0] - cx;
                let dy = pt[1] - cy;
                let rx = (c * dx + s * dy).abs();
                let ry = (-s * dx + c * dy).abs();
                let half_w = pad["width"].as_f64().unwrap() / 2.0;
                let half_h = pad["height"].as_f64().unwrap() / 2.0;
                rx <= half_w + 1e-4 && ry <= half_h + 1e-4
            }
        };

        let layers = |e: &Value| -> Vec<String> {
            let ty = e["type"].as_str().unwrap();
            if ty == "pcb_trace" {
                vec![e["route"][0]["layer"].as_str().unwrap().to_string()]
            } else if ty == "pcb_via" {
                e["layers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|l| l.as_str().unwrap().to_string())
                    .collect()
            } else if ty == "pcb_plated_hole" {
                vec![
                    "top".into(),
                    "bottom".into(),
                    "inner1".into(),
                    "inner2".into(),
                ]
            } else {
                vec![e["layer"].as_str().unwrap().to_string()]
            }
        };

        for i in 0..items.len() {
            for j in i + 1..items.len() {
                let a = items[i];
                let b = items[j];
                let la = layers(a);
                let lb = layers(b);
                if !la.iter().any(|l| lb.contains(l)) {
                    continue;
                }

                let ty_a = a["type"].as_str().unwrap();
                let ty_b = b["type"].as_str().unwrap();

                let connected = match (ty_a, ty_b) {
                    ("pcb_trace", "pcb_trace") => {
                        let r_a = a["route"].as_array().unwrap();
                        let r_b = b["route"].as_array().unwrap();
                        let (a0, a1) = (point(&r_a[0]), point(&r_a[1]));
                        let (b0, b1) = (point(&r_b[0]), point(&r_b[1]));
                        distance(a0, b0) < 1e-4
                            || distance(a0, b1) < 1e-4
                            || distance(a1, b0) < 1e-4
                            || distance(a1, b1) < 1e-4
                    }
                    ("pcb_trace", "pcb_via") | ("pcb_via", "pcb_trace") => {
                        let (tr, v) = if ty_a == "pcb_trace" {
                            (a, b)
                        } else {
                            (b, a)
                        };
                        let r_tr = tr["route"].as_array().unwrap();
                        let (t0, t1) = (point(&r_tr[0]), point(&r_tr[1]));
                        let vp = point(v);
                        distance(t0, vp) < 1e-4 || distance(t1, vp) < 1e-4
                    }
                    ("pcb_trace", "pcb_smtpad")
                    | ("pcb_smtpad", "pcb_trace")
                    | ("pcb_trace", "pcb_plated_hole")
                    | ("pcb_plated_hole", "pcb_trace") => {
                        let (tr, pd) = if ty_a == "pcb_trace" {
                            (a, b)
                        } else {
                            (b, a)
                        };
                        let r_tr = tr["route"].as_array().unwrap();
                        let (t0, t1) = (point(&r_tr[0]), point(&r_tr[1]));
                        pad_contains(pd, t0) || pad_contains(pd, t1)
                    }
                    ("pcb_via", "pcb_smtpad")
                    | ("pcb_smtpad", "pcb_via")
                    | ("pcb_via", "pcb_plated_hole")
                    | ("pcb_plated_hole", "pcb_via") => {
                        let (v, pd) = if ty_a == "pcb_via" {
                            (a, b)
                        } else {
                            (b, a)
                        };
                        let vp = point(v);
                        pad_contains(pd, vp)
                    }
                    ("pcb_plated_hole", "pcb_plated_hole") | ("pcb_smtpad", "pcb_smtpad") => {
                        let (pa, pb) = (point(a), point(b));
                        pad_contains(a, pb) || pad_contains(b, pa)
                    }
                    _ => false,
                };

                if connected {
                    union(&mut parent, i, j);
                }
            }
        }

        let first_pad_root = find(&mut parent, pads[0]);
        for &pad_idx in &pads[1..] {
            let r = find(&mut parent, pad_idx);
            assert_eq!(
                r, first_pad_root,
                "Net {net} is not a single connected group: pad {:?} not connected to pad {:?}",
                items[pad_idx], items[pads[0]]
            );
        }
    }
}

// =============================================================================
// TEST 1: Medium 2-layer board
// =============================================================================
#[tokio::test]
async fn test_1_medium_board_segment_reduction_via_count_length_and_drc() {
    let mut spec_off = medium_board_spec();
    spec_off["rules"]["beautify"] = json!(false);
    let build_off = build(&spec_off);
    let elements_off = elements(&build_off);

    let mut spec_on = medium_board_spec();
    spec_on["rules"]["beautify"] = json!(true);
    let build_on = build(&spec_on);
    let elements_on = elements(&build_on);

    let segs_off = count(&elements_off, "pcb_trace");
    let segs_on = count(&elements_on, "pcb_trace");
    assert!(
        segs_on * 4 <= segs_off * 3,
        "Segment count did not drop by >= 25%: off={segs_off}, on={segs_on}"
    );

    let vias_off = count(&elements_off, "pcb_via");
    let vias_on = count(&elements_on, "pcb_via");
    assert!(
        vias_on <= vias_off,
        "Via count increased: off={vias_off}, on={vias_on}"
    );

    let lens_off = lengths(&elements_off);
    let lens_on = lengths(&elements_on);

    for (net, pads) in spec_on["nets"].as_object().unwrap() {
        let pad_count = pads.as_array().unwrap().len() as f64;
        let l_off = lens_off.get(net).copied().unwrap_or(0.0);
        let l_on = lens_on.get(net).copied().unwrap_or(0.0);
        let max_allowed = l_off + 0.2 * pad_count;
        assert!(
            l_on <= max_allowed + 1e-5,
            "Net {net} length exceeded: on={l_on}, off={l_off}, pads={pad_count}, max_allowed={max_allowed}"
        );
    }

    let drc_off = drc(&spec_off, &build_off).await;
    assert!(
        drc_off.ends_with("No DRC errors."),
        "DRC failed on beautify off: {drc_off}"
    );

    let drc_on = drc(&spec_on, &build_on).await;
    assert!(
        drc_on.ends_with("No DRC errors."),
        "DRC failed on beautify on: {drc_on}"
    );
}

// =============================================================================
// TEST 2: Angles
// =============================================================================
#[tokio::test]
async fn test_2_angles_octilinear_and_no_acute_corners() {
    let mut spec_off = medium_board_spec();
    spec_off["rules"]["beautify"] = json!(false);
    let _build_off = build(&spec_off);

    let mut spec_on = medium_board_spec();
    spec_on["rules"]["beautify"] = json!(true);
    let build_on = build(&spec_on);
    let elem_on = elements(&build_on);

    // 1. Every trace segment longer than 0.6 mm has a direction that is a multiple of 45° (±0.5°)
    for t in elem_on.iter().filter(|e| e["type"] == "pcb_trace") {
        let route = t["route"].as_array().unwrap();
        for w in route.windows(2) {
            let p0 = point(&w[0]);
            let p1 = point(&w[1]);
            let len = distance(p0, p1);
            if len > 0.6 {
                let dx = p1[0] - p0[0];
                let dy = p1[1] - p0[1];
                let deg = dy.atan2(dx).to_degrees().abs();
                let rem = deg % 45.0;
                let diff = rem.min(45.0 - rem);
                assert!(
                    diff <= 0.5,
                    "Segment with len {len:.3} has angle {deg:.2}° (diff {diff:.2}°) which is not multiple of 45° ± 0.5°: {t}"
                );
            }
        }
    }

    // 2. Count consecutive-segment corners with an interior angle < 90° (acute) — must be 0
    let mut acute_corners = 0;
    let traces: Vec<_> = elem_on.iter().filter(|e| e["type"] == "pcb_trace").collect();
    for (i, t1) in traces.iter().enumerate() {
        let r1 = t1["route"].as_array().unwrap();
        let (a0, a1) = (point(&r1[0]), point(&r1[1]));
        let net1 = t1["net"].as_str().unwrap();
        let layer1 = r1[0]["layer"].as_str().unwrap();

        for t2 in &traces[i + 1..] {
            let r2 = t2["route"].as_array().unwrap();
            let net2 = t2["net"].as_str().unwrap();
            let layer2 = r2[0]["layer"].as_str().unwrap();

            if net1 != net2 || layer1 != layer2 {
                continue;
            }

            let (b0, b1) = (point(&r2[0]), point(&r2[1]));

            let pairs = [
                (a0, a1, b0, b1),
                (a0, a1, b1, b0),
                (a1, a0, b0, b1),
                (a1, a0, b1, b0),
            ];

            for (c1, p1, c2, p2) in pairs {
                if distance(c1, c2) < 1e-5 {
                    let v1 = [p1[0] - c1[0], p1[1] - c1[1]];
                    let v2 = [p2[0] - c2[0], p2[1] - c2[1]];
                    let d1 = distance(c1, p1);
                    let d2 = distance(c2, p2);
                    if d1 > 1e-6 && d2 > 1e-6 {
                        let dot = (v1[0] * v2[0] + v1[1] * v2[1]) / (d1 * d2);
                        let interior_angle = dot.clamp(-1.0, 1.0).acos().to_degrees();
                        if interior_angle < 90.0 - 0.5 {
                            acute_corners += 1;
                        }
                    }
                }
            }
        }
    }

    assert_eq!(
        acute_corners, 0,
        "Expected 0 acute corners on beautified board, found {acute_corners}"
    );
}

// =============================================================================
// TEST 3: Connectivity from circuit JSON alone
// =============================================================================
#[tokio::test]
async fn test_3_connectivity_union_find_from_circuit_json() {
    let mut spec_off = medium_board_spec();
    spec_off["rules"]["beautify"] = json!(false);
    let build_off = build(&spec_off);
    let elem_off = elements(&build_off);
    assert_connected_from_circuit_json(&elem_off);

    let mut spec_on = medium_board_spec();
    spec_on["rules"]["beautify"] = json!(true);
    let build_on = build(&spec_on);
    let elem_on = elements(&build_on);
    assert_connected_from_circuit_json(&elem_on);
}

// =============================================================================
// TEST 4: Report line
// =============================================================================
#[tokio::test]
async fn test_4_report_line_format_and_presence() {
    let mut spec_off = medium_board_spec();
    spec_off["rules"]["beautify"] = json!(false);
    let build_off = build(&spec_off);
    assert!(
        !build_off.report.contains("Beautified:"),
        "Report should not contain Beautified line when beautify is off: {}",
        build_off.report
    );

    let mut spec_on = medium_board_spec();
    spec_on["rules"]["beautify"] = json!(true);
    let build_on = build(&spec_on);

    let line = build_on
        .report
        .lines()
        .find(|l| l.starts_with("Beautified:"))
        .expect("Beautified report line missing with beautify on");

    // Format: Beautified: {before} -> {after} segments, {vias} via(s) removed, {chamfers} corner(s) chamfered.
    let parts: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(parts[0], "Beautified:");
    let before: usize = parts[1].parse().expect("before segments integer");
    assert_eq!(parts[2], "->");
    let after: usize = parts[3].parse().expect("after segments integer");
    assert_eq!(parts[4], "segments,");
    let vias: usize = parts[5].parse().expect("vias removed integer");
    assert_eq!(parts[6], "via(s)");
    assert_eq!(parts[7], "removed,");
    let chamfers: usize = parts[8].parse().expect("chamfers integer");
    assert_eq!(parts[9], "corner(s)");
    assert_eq!(parts[10], "chamfered.");

    assert!(before >= after);
}

// =============================================================================
// TEST 5: Default is on and strict boolean
// =============================================================================
#[tokio::test]
async fn test_5_default_is_on_and_strict_boolean() {
    // 1. Spec with no beautify field equals explicit "beautify": true build byte-for-byte
    let spec_default = medium_board_spec();
    let build_default = build(&spec_default);

    let mut spec_true = medium_board_spec();
    spec_true["rules"]["beautify"] = json!(true);
    let build_true = build(&spec_true);

    let mut spec_off = medium_board_spec();
    spec_off["rules"]["beautify"] = json!(false);
    let build_off = build(&spec_off);

    assert_eq!(
        build_default.files, build_true.files,
        "Default build files do not match beautify: true files"
    );
    assert_eq!(
        build_default.binary_files, build_true.binary_files,
        "Default build binary files do not match beautify: true"
    );
    assert_eq!(
        build_default.report, build_true.report,
        "Default build report does not match beautify: true report"
    );
    assert_ne!(
        build_default.files, build_off.files,
        "Default build files should differ from beautify: false files"
    );

    // 2. Strict boolean validation: "beautify": "yes", 1, null must fail to build
    for bad in [json!("yes"), json!(1), Value::Null] {
        let mut bad_spec = medium_board_spec();
        bad_spec["rules"]["beautify"] = bad.clone();
        let res = pcb::build(&bad_spec.to_string(), "test", "pcb/test.pcb.json");
        assert!(
            res.is_err(),
            "Expected build to fail for strict boolean with value: {bad}"
        );
    }
}

// =============================================================================
// TEST 6: 4-layer plane board
// =============================================================================
#[tokio::test]
async fn test_6_four_layer_plane_board_no_tracks_on_inner_layers_and_drc_clean() {
    let spec = json!({
        "board": {"width": 40, "height": 30, "layers": 4},
        "rules": {"clearance": 0.2},
        "parts": [
            {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [0, 0]},
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x4", "at": [0, -8], "rotation": 90},
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [-10, 5]},
            {"ref": "C1", "kind": "capacitor", "footprint": "0603", "at": [10, 5]}
        ],
        "nets": {
            "GND": ["J1.1", "U1.1"],
            "3V3": ["J1.2", "U1.8"],
            "SIG1": ["J1.3", "R1.1", "U1.2"],
            "SIG2": ["J1.4", "C1.1", "U1.7"]
        },
        "pours": [
            {"net": "GND", "layer": "inner1"},
            {"net": "3V3", "layer": "inner2"}
        ]
    });

    let mut spec_off = spec.clone();
    spec_off["rules"]["beautify"] = json!(false);
    let build_off = build(&spec_off);
    let drc_off = drc(&spec_off, &build_off).await;
    assert!(
        drc_off.ends_with("No DRC errors."),
        "DRC failed on beautify off: {drc_off}"
    );

    let mut spec_on = spec.clone();
    spec_on["rules"]["beautify"] = json!(true);
    let build_on = build(&spec_on);
    let elem_on = elements(&build_on);

    for t in elem_on.iter().filter(|e| e["type"] == "pcb_trace") {
        for pt in t["route"].as_array().unwrap() {
            let l = pt["layer"].as_str().unwrap();
            assert!(
                l != "inner1" && l != "inner2",
                "Trace point found on inner layer {l}: {t}"
            );
        }
    }

    let drc_on = drc(&spec_on, &build_on).await;
    assert!(
        drc_on.ends_with("No DRC errors."),
        "DRC failed on beautify on: {drc_on}"
    );
}

// =============================================================================
// TEST 7: Diffpair and 3-net length group
// =============================================================================
#[tokio::test]
async fn test_7_diffpair_and_length_group_quality_metrics() {
    let spec = json!({
        "board": {"width": 50, "height": 45, "layers": 4},
        "rules": {"clearance": 0.15},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "usb_micro_b", "at": [-14, -10], "rotation": -90},
            {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [14, -10]},
            {"ref": "A0", "kind": "resistor", "footprint": "0603", "at": [-16, 0]},
            {"ref": "B0", "kind": "resistor", "footprint": "0603", "at": [8, 0]},
            {"ref": "A1", "kind": "resistor", "footprint": "0603", "at": [-16, 8]},
            {"ref": "B1", "kind": "resistor", "footprint": "0603", "at": [12, 8]},
            {"ref": "A2", "kind": "resistor", "footprint": "0603", "at": [-16, 16]},
            {"ref": "B2", "kind": "resistor", "footprint": "0603", "at": [10, 16]}
        ],
        "nets": {
            "DP": ["J1.3", "U1.2"],
            "DN": ["J1.2", "U1.1"],
            "D0": ["A0.2", "B0.1"],
            "D1": ["A1.2", "B1.1"],
            "D2": ["A2.2", "B2.1"]
        },
        "diffpairs": {
            "USB": {"p": "DP", "n": "DN", "gap": 0.15, "width": 0.2}
        },
        "length_groups": {
            "DATA": {"nets": ["D0", "D1", "D2"], "tolerance": 0.5}
        }
    });

    for beautify in [false, true] {
        let mut s = spec.clone();
        s["rules"]["beautify"] = json!(beautify);
        let b = build(&s);

        // Diffpair checks: coupled % >= 80, skew <= 0.10
        let dp_line = b
            .report
            .lines()
            .find(|l| l.starts_with("Diffpair USB:"))
            .expect("Diffpair USB report line missing");

        // Format: Diffpair USB: coupled {X}% of ... skew {Y} mm ...
        let coupled_str = dp_line
            .split("coupled ")
            .nth(1)
            .unwrap()
            .split('%')
            .next()
            .unwrap();
        let coupled: f64 = coupled_str.parse().expect("coupled % float");
        assert!(
            coupled >= 80.0,
            "Diffpair coupled % < 80 (got {coupled}%): {dp_line} (beautify={beautify})"
        );

        let skew_str = dp_line
            .split("skew ")
            .nth(1)
            .unwrap()
            .split(" mm")
            .next()
            .unwrap();
        let skew: f64 = skew_str.parse().expect("skew float");
        assert!(
            skew <= 0.10 + 1e-4,
            "Diffpair skew > 0.10 (got {skew}): {dp_line} (beautify={beautify})"
        );

        // Length group check: spread <= 0.5
        let lg_line = b
            .report
            .lines()
            .find(|l| l.starts_with("Length group DATA:"))
            .expect("Length group DATA report line missing");

        // Format: Length group DATA: {min}-{max} mm, tolerance 0.50 mm
        let range_str = lg_line
            .strip_prefix("Length group DATA: ")
            .unwrap()
            .split(" mm")
            .next()
            .unwrap();
        let range_parts: Vec<&str> = range_str.split('-').collect();
        let min_len: f64 = range_parts[0].parse().expect("min length float");
        let max_len: f64 = range_parts[1].parse().expect("max length float");
        let spread = max_len - min_len;
        assert!(
            spread <= 0.50 + 1e-4,
            "Length group spread > 0.5 (got {spread}): {lg_line} (beautify={beautify})"
        );

        let drc_rep = drc(&s, &b).await;
        assert!(
            drc_rep.ends_with("No DRC errors."),
            "DRC failed on beautify={beautify}: {drc_rep}"
        );
    }
}

// =============================================================================
// TEST 8: Determinism
// =============================================================================
#[tokio::test]
async fn test_8_determinism_identical_repeated_builds() {
    let mut spec_off = medium_board_spec();
    spec_off["rules"]["beautify"] = json!(false);
    let _build_off = build(&spec_off);

    let mut spec_on = medium_board_spec();
    spec_on["rules"]["beautify"] = json!(true);

    let build_1 = build(&spec_on);
    let build_2 = build(&spec_on);

    assert_eq!(
        build_1.files, build_2.files,
        "Non-deterministic build files with beautify on"
    );
    assert_eq!(
        build_1.binary_files, build_2.binary_files,
        "Non-deterministic binary files with beautify on"
    );
    assert_eq!(
        build_1.report, build_2.report,
        "Non-deterministic report with beautify on"
    );
}

// =============================================================================
// TEST 9: Small hand-made diagonal resistors
// =============================================================================
#[tokio::test]
async fn test_9_small_diagonal_two_resistors_segments_and_length() {
    let spec = json!({
        "board": {"width": 30, "height": 20, "layers": 2},
        "parts": [
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [-3.5355, -3.5355]},
            {"ref": "R2", "kind": "resistor", "footprint": "0603", "at": [3.5355, 3.5355]}
        ],
        "nets": {"NET": ["R1.2", "R2.1"]}
    });

    let mut spec_off = spec.clone();
    spec_off["rules"] = json!({"beautify": false});
    let build_off = build(&spec_off);
    let elem_off = elements(&build_off);

    let mut spec_on = spec.clone();
    spec_on["rules"] = json!({"beautify": true});
    let build_on = build(&spec_on);
    let elem_on = elements(&build_on);

    let segs_on = count(&elem_on, "pcb_trace");
    let len_off = lengths(&elem_off)["NET"];
    let len_on = lengths(&elem_on)["NET"];

    assert!(
        segs_on <= 3,
        "Expected <= 3 segments on beautify on, got {segs_on}"
    );
    assert!(
        len_on <= len_off + 1e-5,
        "Expected beautify on route to be no longer than off: on={len_on}, off={len_off}"
    );

    let drc_off = drc(&spec_off, &build_off).await;
    assert!(
        drc_off.ends_with("No DRC errors."),
        "DRC failed on beautify off: {drc_off}"
    );

    let drc_on = drc(&spec_on, &build_on).await;
    assert!(
        drc_on.ends_with("No DRC errors."),
        "DRC failed on beautify on: {drc_on}"
    );
}
