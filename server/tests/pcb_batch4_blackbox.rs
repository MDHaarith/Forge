mod common;
#[path = "common/tools.rs"]
mod support;

use std::collections::BTreeSet;
use forge_server::tools::pcb::{self, NativeBuild};
use serde_json::{json, Value};

fn circuit(b: &NativeBuild) -> Vec<Value> {
    serde_json::from_str::<Value>(&b.files[0].1).unwrap().as_array().unwrap().clone()
}

fn length(elements: &[Value], net: &str) -> f64 {
    elements
        .iter()
        .filter(|e| e["type"] == "pcb_trace" && e["net"] == net)
        .map(|e| {
            e["route"]
                .as_array()
                .unwrap()
                .windows(2)
                .map(|p| {
                    (p[0]["x"].as_f64().unwrap() - p[1]["x"].as_f64().unwrap())
                        .hypot(p[0]["y"].as_f64().unwrap() - p[1]["y"].as_f64().unwrap())
                })
                .sum::<f64>()
        })
        .sum()
}

async fn drc(spec: &Value, built: &NativeBuild) -> String {
    let dir = support::project(
        "pcb-batch4-bb",
        &[
            ("pcb/test.pcb.json", spec.to_string()),
            ("pcb/test.circuit.json", built.files[0].1.clone()),
        ],
    );
    support::run("pcb_drc", json!({"board":"test"}), &dir.path)
        .await
        .unwrap()
        .text
}

fn err_msg(res: Result<NativeBuild, String>) -> String {
    match res {
        Ok(_) => panic!("expected Err, but build succeeded"),
        Err(e) => e,
    }
}

#[test]
fn test_impedance_single_ended_and_limits() {
    let spec_4l = json!({
        "board": {"width": 30, "height": 20, "layers": 4},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x1", "at": [-10, 0]},
            {"ref": "J2", "kind": "header", "footprint": "pinheader_1x1", "at": [10, 0]}
        ],
        "nets": {"ANT": ["J1.1", "J2.1"]},
        "netclasses": {"rf": {"nets": ["ANT"], "impedance": 50}}
    });
    let built_4l = pcb::build(&spec_4l.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements_4l = circuit(&built_4l);
    let trace_4l = elements_4l
        .iter()
        .find(|e| e["type"] == "pcb_trace" && e["net"] == "ANT")
        .expect("ANT trace missing");
    let width_4l = trace_4l["width"].as_f64().unwrap();
    assert!(
        (width_4l - 0.344).abs() <= 0.003,
        "expected width 0.344 ± 0.003 mm, got {width_4l}"
    );
    let report_line = built_4l
        .report
        .lines()
        .find(|l| l.starts_with("Impedance: "))
        .expect("Impedance line missing from build report");
    assert!(
        report_line.contains("rf 50 ohm -> "),
        "expected 'rf 50 ohm -> ' in report line: {report_line}"
    );

    // Same on 2 layers, 1.6 mm thickness
    let mut spec_2l = spec_4l.clone();
    spec_2l["board"]["layers"] = json!(2);
    let built_2l = pcb::build(&spec_2l.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements_2l = circuit(&built_2l);
    let trace_2l = elements_2l
        .iter()
        .find(|e| e["type"] == "pcb_trace" && e["net"] == "ANT")
        .expect("ANT trace missing on 2-layer board");
    let width_2l = trace_2l["width"].as_f64().unwrap();
    assert!(
        width_2l > 2.0,
        "expected 2-layer 50 ohm width > 2.0 mm, got {width_2l}"
    );

    // 2-layer 10 ohm target error
    let mut spec_10 = spec_2l.clone();
    spec_10["netclasses"]["rf"]["impedance"] = json!(10);
    let err_10 = err_msg(pcb::build(&spec_10.to_string(), "test", "pcb/test.pcb.json"));
    assert!(
        err_10.contains("impedance 10 ohm needs a trace width outside 0.05-5 mm"),
        "unexpected error message: {err_10}"
    );

    // 2-layer 200 ohm target error
    let mut spec_200 = spec_2l.clone();
    spec_200["netclasses"]["rf"]["impedance"] = json!(200);
    let err_200 = err_msg(pcb::build(&spec_200.to_string(), "test", "pcb/test.pcb.json"));
    assert!(
        err_200.contains("impedance 200 ohm needs a trace width outside 0.05-5 mm"),
        "unexpected error message: {err_200}"
    );
}

#[test]
fn test_spec_validation_errors() {
    let base = json!({
        "board": {"width": 30, "height": 20, "layers": 4},
        "parts": [
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [-5, 0]},
            {"ref": "R2", "kind": "resistor", "footprint": "0603", "at": [5, 0]}
        ],
        "nets": {"N1": ["R1.1", "R2.1"], "N2": ["R1.2", "R2.2"]}
    });

    // 1. netclass {name}: set trace or impedance, not both
    let mut s = base.clone();
    s["netclasses"] = json!({"rf": {"nets": ["N1"], "trace": 0.2, "impedance": 50}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("netclass rf: set trace or impedance, not both")
    );

    // 2. netclass {name}: set trace or impedance
    let mut s = base.clone();
    s["netclasses"] = json!({"rf": {"nets": ["N1"]}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("netclass rf: set trace or impedance")
    );

    // 3. diffpair {name}: unknown net {net}
    let mut s = base.clone();
    s["diffpairs"] = json!({"DP": {"p": "UNKNOWN", "n": "N2"}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("diffpair DP: unknown net UNKNOWN")
    );

    // 4. diffpair {name}: net {net} must connect exactly 2 pins
    let mut s = base.clone();
    s["nets"]["N1"] = json!(["R1.1"]);
    s["diffpairs"] = json!({"DP": {"p": "N1", "n": "N2"}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("diffpair DP: net N1 must connect exactly 2 pins")
    );

    // 5. diffpair {name}: net {net} is already in another diffpair
    let mut s = base.clone();
    s["diffpairs"] = json!({"DP1": {"p": "N1", "n": "N2"}, "DP2": {"p": "N1", "n": "N2"}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("diffpair DP2: net N1 is already in another diffpair")
    );

    // 6. diffpair {name}: set width or impedance, not both
    let mut s = base.clone();
    s["diffpairs"] = json!({"DP": {"p": "N1", "n": "N2", "width": 0.2, "impedance": 90}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("diffpair DP: set width or impedance, not both")
    );

    // 7. diffpair {name}: gap {gap} is below clearance {clearance}
    let mut s = base.clone();
    s["rules"] = json!({"clearance": 0.15});
    s["diffpairs"] = json!({"DP": {"p": "N1", "n": "N2", "gap": 0.1}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("diffpair DP: gap 0.1 is below clearance 0.15")
    );

    // 8. length group {name}: unknown net {net}
    let mut s = base.clone();
    s["length_groups"] = json!({"GRP": {"nets": ["BAD", "N1"], "tolerance": 0.5}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("length group GRP: unknown net BAD")
    );

    // 9. length group {name}: needs at least 2 nets
    let mut s = base.clone();
    s["length_groups"] = json!({"GRP": {"nets": ["N1"], "tolerance": 0.5}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("length group GRP: needs at least 2 nets")
    );

    // 10. length group {name}: net {net} is in diffpair {pair}
    let mut s = base.clone();
    s["diffpairs"] = json!({"DP": {"p": "N1", "n": "N2"}});
    s["length_groups"] = json!({"GRP": {"nets": ["N1", "N2"], "tolerance": 0.5}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("length group GRP: net N1 is in diffpair DP")
    );

    // 11. length group {name}: net {net} is already in another length group
    let mut s = base.clone();
    s["length_groups"] = json!({
        "G1": {"nets": ["N1", "N2"], "tolerance": 0.5},
        "G2": {"nets": ["N1", "N2"], "tolerance": 0.5}
    });
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("length group G2: net N1 is already in another length group")
    );

    // 12. rules.blind_vias needs 4 layers
    let mut s = base.clone();
    s["board"]["layers"] = json!(2);
    s["rules"] = json!({"blind_vias": true});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("rules.blind_vias needs 4 layers")
    );

    // 13. positive checks: board.er, board.prepreg, diffpair.gap, diffpair.width, diffpair.impedance, diffpair.skew, netclass.impedance, length_group.tolerance
    let mut s = base.clone();
    s["board"]["er"] = json!(0);
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("board.er must be positive")
    );

    let mut s = base.clone();
    s["board"]["prepreg"] = json!(-0.21);
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("board.prepreg must be positive")
    );

    let mut s = base.clone();
    s["diffpairs"] = json!({"DP": {"p": "N1", "n": "N2", "gap": 0}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("diffpair.gap must be positive")
    );

    let mut s = base.clone();
    s["diffpairs"] = json!({"DP": {"p": "N1", "n": "N2", "width": -0.2}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("diffpair.width must be positive")
    );

    let mut s = base.clone();
    s["diffpairs"] = json!({"DP": {"p": "N1", "n": "N2", "impedance": 0}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("diffpair.impedance must be positive")
    );

    let mut s = base.clone();
    s["diffpairs"] = json!({"DP": {"p": "N1", "n": "N2", "skew": 0}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("diffpair.skew must be positive")
    );

    let mut s = base.clone();
    s["netclasses"] = json!({"rf": {"nets": ["N1"], "impedance": 0}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("netclass.impedance must be positive")
    );

    let mut s = base.clone();
    s["length_groups"] = json!({"GRP": {"nets": ["N1", "N2"], "tolerance": -0.5}});
    assert!(
        err_msg(pcb::build(&s.to_string(), "t", "pcb/t.pcb.json"))
            .contains("length_group.tolerance must be positive")
    );
}

#[tokio::test]
async fn test_diffpair_coupled_and_matched_on_clean_board() {
    let spec = json!({
        "board": {"width": 40, "height": 25, "layers": 4},
        "rules": {"clearance": 0.15},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "usb_micro_b", "at": [-14, 0], "rotation": -90},
            {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [14, 0]}
        ],
        "nets": {
            "USB_DP": ["J1.3", "U1.2"],
            "USB_DN": ["J1.2", "U1.1"]
        },
        "diffpairs": {
            "USB": {"p": "USB_DP", "n": "USB_DN", "gap": 0.15, "width": 0.2}
        }
    });
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let line = built
        .report
        .lines()
        .find(|l| l.starts_with("Diffpair USB:"))
        .expect("Diffpair USB line missing from report");
    assert!(line.contains("coupled "), "line: {line}");

    let percent: f64 = line
        .split("coupled ")
        .nth(1)
        .unwrap()
        .split('%')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(percent >= 80.0, "expected coupled >= 80%, got {percent}%");

    let skew_str = line
        .split("skew ")
        .nth(1)
        .unwrap()
        .split(" mm")
        .next()
        .unwrap();
    let skew: f64 = skew_str.parse().unwrap();
    assert!(skew <= 0.10, "expected skew in line <= 0.10 mm, got {skew}");

    let drc_report = drc(&spec, &built).await;
    assert!(
        drc_report.ends_with("No DRC errors."),
        "DRC failed: {drc_report}"
    );

    let elements = circuit(&built);
    let len_p = length(&elements, "USB_DP");
    let len_n = length(&elements, "USB_DN");
    let diff = (len_p - len_n).abs();
    assert!(
        diff <= 0.100001,
        "expected |len(P) - len(N)| <= 0.1 mm, got {diff} mm (P: {len_p}, N: {len_n})"
    );

    let p_layers: BTreeSet<&str> = elements
        .iter()
        .filter(|e| e["type"] == "pcb_trace" && e["net"] == "USB_DP")
        .map(|e| e["route"][0]["layer"].as_str().unwrap())
        .collect();
    let n_layers: BTreeSet<&str> = elements
        .iter()
        .filter(|e| e["type"] == "pcb_trace" && e["net"] == "USB_DN")
        .map(|e| e["route"][0]["layer"].as_str().unwrap())
        .collect();
    assert_eq!(p_layers, n_layers, "P and N traces must be on the same layers");
}

#[tokio::test]
async fn test_diffpair_with_blocking_part_routes_cleanly() {
    let spec = json!({
        "board": {"width": 40, "height": 25, "layers": 4},
        "rules": {"clearance": 0.15},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "usb_micro_b", "at": [-14, 0], "rotation": -90},
            {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [14, 0]},
            {"ref": "U2", "kind": "chip", "footprint": "soic16", "at": [0, 0]}
        ],
        "nets": {
            "USB_DP": ["J1.3", "U1.2"],
            "USB_DN": ["J1.2", "U1.1"]
        },
        "diffpairs": {
            "USB": {"p": "USB_DP", "n": "USB_DN", "gap": 0.15, "width": 0.2}
        }
    });
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let drc_report = drc(&spec, &built).await;
    assert!(
        drc_report.ends_with("No DRC errors."),
        "DRC failed: {drc_report}"
    );
    assert!(
        !drc_report.contains("unrouted:"),
        "found unrouted nets: {drc_report}"
    );
    let elements = circuit(&built);
    assert!(
        length(&elements, "USB_DP") > 0.0,
        "USB_DP trace length is zero"
    );
    assert!(
        length(&elements, "USB_DN") > 0.0,
        "USB_DN trace length is zero"
    );
}

#[tokio::test]
async fn test_diffpair_crossing_pads_fall_back_cleanly() {
    let spec = json!({
        "board": {"width": 40, "height": 25, "layers": 4},
        "rules": {"clearance": 0.15},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "usb_micro_b", "at": [-14, 0], "rotation": -90},
            {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [14, 0]}
        ],
        "nets": {
            "USB_DP": ["J1.3", "U1.1"],
            "USB_DN": ["J1.2", "U1.2"]
        },
        "diffpairs": {
            "USB": {"p": "USB_DP", "n": "USB_DN", "gap": 0.15, "width": 0.2}
        }
    });
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let coupled_line = built.report.contains("Diffpair USB: coupled");
    let fallback_line = built
        .report
        .contains("Diffpair USB: routed as separate nets (could not couple)");
    assert!(
        coupled_line || fallback_line,
        "expected coupled or fallback report line, got: {}",
        built.report
    );
    let drc_report = drc(&spec, &built).await;
    assert!(
        drc_report.ends_with("No DRC errors."),
        "DRC failed: {drc_report}"
    );
    assert!(
        !drc_report.contains("unrouted:"),
        "unrouted nets found: {drc_report}"
    );
}

#[tokio::test]
async fn test_length_group_header_to_chip_meanders_and_matches() {
    let mut spec = json!({
        "board": {"width": 60, "height": 40},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x4", "at": [-18, 0]},
            {"ref": "U1", "kind": "chip", "footprint": "soic16", "at": [15, 0]}
        ],
        "nets": {
            "D0": ["J1.1", "U1.1"],
            "D1": ["J1.2", "U1.4"],
            "D2": ["J1.3", "U1.9"],
            "D3": ["J1.4", "U1.16"]
        },
        "length_groups": {
            "DATA": {"nets": ["D0", "D1", "D2", "D3"], "tolerance": 0.5}
        }
    });

    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let report_line = built
        .report
        .lines()
        .find(|l| l.starts_with("Length group DATA:"))
        .expect("Length group line missing in report");
    assert!(
        report_line.contains("tolerance 0.50 mm"),
        "line: {report_line}"
    );

    // Extract a-b mm
    let range_str = report_line
        .split("Length group DATA: ")
        .nth(1)
        .unwrap()
        .split(" mm")
        .next()
        .unwrap();
    let parts: Vec<&str> = range_str.split('-').collect();
    assert_eq!(parts.len(), 2, "range_str: {range_str}");
    let a: f64 = parts[0].parse().unwrap();
    let b: f64 = parts[1].parse().unwrap();
    assert!(
        b - a <= 0.500001,
        "report spread b - a must be <= 0.5 mm, got {}",
        b - a
    );

    // Recomputed lengths from circuit JSON
    let elements = circuit(&built);
    let lengths: Vec<f64> = ["D0", "D1", "D2", "D3"]
        .iter()
        .map(|n| length(&elements, n))
        .collect();
    let recomputed_min = lengths.iter().copied().fold(f64::INFINITY, f64::min);
    let recomputed_max = lengths.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let recomputed_spread = recomputed_max - recomputed_min;

    assert!(
        recomputed_spread <= 0.500001,
        "recomputed spread must be <= 0.5 mm, got {recomputed_spread}"
    );
    assert!(
        (recomputed_spread - (b - a)).abs() <= 0.010001,
        "recomputed spread {recomputed_spread} should agree within 0.01 mm of report spread {}",
        b - a
    );
    assert!(
        (recomputed_min - a).abs() <= 0.010001,
        "min length {recomputed_min} should agree within 0.01 mm with report {a}"
    );
    assert!(
        (recomputed_max - b).abs() <= 0.010001,
        "max length {recomputed_max} should agree within 0.01 mm with report {b}"
    );

    // DRC check
    let drc_report = drc(&spec, &built).await;
    assert!(
        drc_report.ends_with("No DRC errors."),
        "DRC failed: {drc_report}"
    );

    // Same board without the group has fewer track points (meanders added)
    spec.as_object_mut().unwrap().remove("length_groups");
    let built_natural = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements_natural = circuit(&built_natural);

    let points_with_group: usize = elements
        .iter()
        .filter(|e| e["type"] == "pcb_trace")
        .map(|e| e["route"].as_array().unwrap().len())
        .sum();
    let points_without_group: usize = elements_natural
        .iter()
        .filter(|e| e["type"] == "pcb_trace")
        .map(|e| e["route"].as_array().unwrap().len())
        .sum();
    assert!(
        points_without_group < points_with_group,
        "expected fewer track points without group ({points_without_group}) than with meanders ({points_with_group})"
    );
}

#[tokio::test]
async fn test_length_group_drc_flags_exceeded_tolerance() {
    let spec = json!({
        "board": {"width": 60, "height": 40},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x4", "at": [-18, 0]},
            {"ref": "U1", "kind": "chip", "footprint": "soic16", "at": [15, 0]}
        ],
        "nets": {
            "D0": ["J1.1", "U1.1"],
            "D1": ["J1.2", "U1.4"],
            "D2": ["J1.3", "U1.9"],
            "D3": ["J1.4", "U1.16"]
        },
        "length_groups": {
            "DATA": {"nets": ["D0", "D1", "D2", "D3"], "tolerance": 0.5}
        }
    });

    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let mut elements = circuit(&built);

    let t = elements
        .iter()
        .find(|e| e["type"] == "pcb_trace" && e["net"] == "D0")
        .expect("D0 trace missing");
    let p = &t["route"][0];
    let x = p["x"].as_f64().unwrap();
    elements.push(json!({
        "type": "pcb_trace",
        "width": 0.2,
        "net": "D0",
        "route": [p.clone(), {"x": x + 3.0, "y": p["y"], "width": 0.2, "layer": p["layer"]}]
    }));

    let mut built_edited = built;
    built_edited.files[0].1 = serde_json::to_string(&elements).unwrap();

    let drc_report = drc(&spec, &built_edited).await;
    assert!(
        drc_report.contains("length: group DATA spread"),
        "expected 'length: group DATA spread' in report: {drc_report}"
    );
    assert!(
        drc_report.contains("exceeds tolerance 0.50 mm"),
        "expected 'exceeds tolerance 0.50 mm' in report: {drc_report}"
    );
}

#[tokio::test]
async fn test_blind_vias_spans_drill_files_and_rules() {
    let spec = json!({
        "board": {"width": 24, "height": 16, "layers": 4},
        "rules": {"blind_vias": true},
        "parts": [
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [-6, 0]},
            {"ref": "R2", "kind": "resistor", "footprint": "0603", "at": [6, 0], "side": "bottom"}
        ],
        "nets": {
            "GND": ["R1.1", "R2.1"],
            "SIG": ["R1.2", "R2.2"]
        },
        "pours": [{"net": "GND", "layer": "inner1"}]
    });

    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&built);
    let vias: Vec<_> = elements
        .iter()
        .filter(|e| e["type"] == "pcb_via")
        .collect();

    // At least one via whose layers is not all four
    assert!(
        vias.iter().any(|v| v["layers"].as_array().unwrap().len() < 4),
        "expected at least one blind/buried via with fewer than 4 layers"
    );

    // A file ending in -PTH-1-2.drl (or another span) containing Plated and Blind or Buried
    let span_file = built
        .files
        .iter()
        .find(|(n, _)| {
            n.contains("-PTH-") && !n.ends_with("-PTH.drl") && n.ends_with(".drl")
        })
        .expect("expected span drill file ending in -PTH-{a}-{b}.drl");
    let (_, span_content) = span_file;
    assert!(
        span_content.contains("TF.FileFunction,Plated,"),
        "missing Plated FileFunction in {}: {}",
        span_file.0,
        span_content
    );
    assert!(
        span_content.contains("Blind") || span_content.contains("Buried"),
        "missing Blind or Buried in {}: {}",
        span_file.0,
        span_content
    );

    // Every via in a span file is absent from -PTH.drl
    let pth_file = built
        .files
        .iter()
        .find(|(n, _)| n.ends_with("-PTH.drl"))
        .expect("missing -PTH.drl file");
    let pth_content = &pth_file.1;
    for (name, content) in &built.files {
        if name.contains("-PTH-") && !name.ends_with("-PTH.drl") && name.ends_with(".drl") {
            for line in content.lines().filter(|l| l.starts_with('X')) {
                assert!(
                    !pth_content.contains(line),
                    "via drill {line} from {name} must be absent from -PTH.drl"
                );
            }
        }
    }

    // DRC clean
    let drc_report = drc(&spec, &built).await;
    assert!(
        drc_report.ends_with("No DRC errors."),
        "DRC failed: {drc_report}"
    );

    // blind_vias: true on 2-layer board errors
    let mut spec_2l = spec.clone();
    spec_2l["board"]["layers"] = json!(2);
    let err = err_msg(pcb::build(&spec_2l.to_string(), "test", "pcb/test.pcb.json"));
    assert!(
        err.contains("rules.blind_vias needs 4 layers"),
        "unexpected error message: {err}"
    );
}

#[tokio::test]
async fn test_teardrops_generate_copper_and_default_off() {
    let mut spec = json!({
        "board": {"width": 25, "height": 18, "layers": 2},
        "rules": {"trace": 0.2, "teardrops": true},
        "parts": [
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [-5, 0]},
            {"ref": "R2", "kind": "resistor", "footprint": "0603", "at": [5, 0]}
        ],
        "nets": {"N": ["R1.2", "R2.1"]}
    });

    let on = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements_on = circuit(&on);
    let teardrops: Vec<_> = elements_on
        .iter()
        .filter(|e| e["type"] == "pcb_trace_teardrop")
        .collect();
    assert!(
        !teardrops.is_empty(),
        "expected pcb_trace_teardrop elements when teardrops rule is true"
    );
    for td in &teardrops {
        assert!(td.get("layer").is_some(), "missing layer in teardrop");
        assert!(td.get("net").is_some(), "missing net in teardrop");
        let pts = td["points"].as_array().expect("points must be array");
        assert!(
            pts.len() >= 3,
            "teardrop polygon must have >= 3 points, got {}",
            pts.len()
        );
    }

    let drc_report = drc(&spec, &on).await;
    assert!(
        drc_report.ends_with("No DRC errors."),
        "DRC failed: {drc_report}"
    );

    spec["rules"]["teardrops"] = json!(false);
    let off = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements_off = circuit(&off);
    let teardrops_off: Vec<_> = elements_off
        .iter()
        .filter(|e| e["type"] == "pcb_trace_teardrop")
        .collect();
    assert_eq!(
        teardrops_off.len(),
        0,
        "expected zero pcb_trace_teardrop elements with teardrops off"
    );

    let gbr_on = on.files.iter().find(|(n, _)| n.ends_with("F_Cu.gbr")).unwrap();
    let gbr_off = off.files.iter().find(|(n, _)| n.ends_with("F_Cu.gbr")).unwrap();
    let regions_on = gbr_on.1.matches("G36*").count();
    let regions_off = gbr_off.1.matches("G36*").count();
    assert!(
        regions_on > regions_off,
        "expected more G36* regions with teardrops on ({regions_on}) than off ({regions_off})"
    );
}

#[test]
fn test_defaults_produce_identical_output() {
    let clean_spec = json!({
        "board": {"width": 30, "height": 20, "layers": 4},
        "parts": [
            {"ref": "R1", "kind": "resistor", "footprint": "0402", "at": [-5, 0]},
            {"ref": "R2", "kind": "resistor", "footprint": "0402", "at": [5, 0], "side": "bottom"}
        ],
        "nets": {"N": ["R1.1", "R2.1"]},
        "holes": [{"at": [0, 5], "d": 2}],
        "pours": [{"net": "GND", "layer": "inner1"}]
    });

    let built_clean = pcb::build(&clean_spec.to_string(), "test", "pcb/test.pcb.json").unwrap();

    let mut explicit_spec = clean_spec.clone();
    explicit_spec["rules"] = json!({"blind_vias": false, "teardrops": false});
    explicit_spec["diffpairs"] = json!({});
    explicit_spec["length_groups"] = json!({});
    explicit_spec["board"]["er"] = json!(4.5);
    explicit_spec["board"]["prepreg"] = json!(0.21);

    let built_explicit =
        pcb::build(&explicit_spec.to_string(), "test", "pcb/test.pcb.json").unwrap();

    assert_eq!(built_clean.report, built_explicit.report);
    assert_eq!(built_clean.files, built_explicit.files);
    assert_eq!(built_clean.binary_files, built_explicit.binary_files);
}

#[test]
fn test_determinism_diffpair_and_length_group_identical_builds() {
    let spec = json!({
        "board": {"width": 70, "height": 45, "layers": 4},
        "rules": {"clearance": 0.15},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "usb_micro_b", "at": [-20, 10], "rotation": -90},
            {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [10, 10]},
            {"ref": "J2", "kind": "header", "footprint": "pinheader_1x4", "at": [-20, -10]},
            {"ref": "U2", "kind": "chip", "footprint": "soic16", "at": [15, -10]}
        ],
        "nets": {
            "USB_DP": ["J1.3", "U1.2"],
            "USB_DN": ["J1.2", "U1.1"],
            "D0": ["J2.1", "U2.1"],
            "D1": ["J2.2", "U2.4"],
            "D2": ["J2.3", "U2.9"],
            "D3": ["J2.4", "U2.16"]
        },
        "diffpairs": {
            "USB": {"p": "USB_DP", "n": "USB_DN", "gap": 0.15, "width": 0.2}
        },
        "length_groups": {
            "DATA": {"nets": ["D0", "D1", "D2", "D3"], "tolerance": 0.5}
        }
    });

    let b1 = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let b2 = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();

    assert_eq!(b1.report, b2.report, "build reports differed");
    assert_eq!(b1.files, b2.files, "build text files differed");
    assert_eq!(b1.binary_files, b2.binary_files, "build binary files differed");
}
