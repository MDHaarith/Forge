mod common;
#[path = "common/tools.rs"]
mod support;

use serde_json::{json, Value};
use support::{project, run};

// =============================================================================
// Reference Board JSONs
// =============================================================================

/// 4-layer board with inner1 as GND plane layer.
/// Contains:
/// - Through-hole pinheader J1 (pin 1 on GND, pins 2..4 on signals)
/// - Three SMD 0603 capacitors (C1, C2, C3) each with pin 2 on GND, pin 1 on signal
/// - Two SMD 0603 resistors (R_TOP on top, R_BOT on bottom) connected together
///   to force a via for a non-GND net (VIA_NET), cutting an antipad in inner1.
const FOUR_LAYER_PLANE_BOARD_JSON: &str = r#"{
  "board": {"width": 60, "height": 40, "layers": 4, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "parts": [
    {"ref": "J1", "kind": "header", "footprint": "pinheader_1x4", "at": [-20, 0]},
    {"ref": "C1", "kind": "capacitor", "footprint": "0603", "at": [-5, 10]},
    {"ref": "C2", "kind": "capacitor", "footprint": "0603", "at": [10, 10]},
    {"ref": "C3", "kind": "capacitor", "footprint": "0603", "at": [20, 10]},
    {"ref": "R_TOP", "kind": "resistor", "footprint": "0603", "at": [0, -10]},
    {"ref": "R_BOT", "kind": "resistor", "footprint": "0603", "at": [15, -10], "side": "bottom"}
  ],
  "nets": {
    "GND": ["J1.1", "C1.2", "C2.2", "C3.2"],
    "SIG1": ["J1.2", "C1.1"],
    "SIG2": ["J1.3", "C2.1"],
    "SIG3": ["J1.4", "C3.1"],
    "VIA_NET": ["R_TOP.1", "R_BOT.1"],
    "BIAS": ["R_TOP.2", "R_BOT.2"]
  },
  "pours": [{"net": "GND", "layer": "inner1"}]
}"#;

/// 2-layer board with an outer-layer pour on "bottom".
const OUTER_LAYER_POUR_BOARD_JSON: &str = r#"{
  "board": {"width": 40, "height": 30, "layers": 2, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "parts": [
    {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [0, -10]},
    {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [-10, 5]},
    {"ref": "R2", "kind": "resistor", "footprint": "0603", "at": [10, 5]}
  ],
  "nets": {
    "GND": ["J1.2", "R1.2", "R2.2"],
    "SIG": ["J1.1", "R1.1", "R2.1"]
  },
  "pours": [{"net": "GND", "layer": "bottom"}]
}"#;

/// Small multi-net board (10 parts, 14 nets, 4 layers with GND plane) for determinism.
const DETERMINISM_BOARD_JSON: &str = r#"{
  "board": {"width": 50, "height": 40, "layers": 4, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "parts": [
    {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [-12, 6]},
    {"ref": "U2", "kind": "chip", "footprint": "soic8", "at": [12, 6]},
    {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [-12, -6]},
    {"ref": "R2", "kind": "resistor", "footprint": "0603", "at": [-6, -6]},
    {"ref": "R3", "kind": "resistor", "footprint": "0603", "at": [0, -6]},
    {"ref": "R4", "kind": "resistor", "footprint": "0603", "at": [6, -6]},
    {"ref": "R5", "kind": "resistor", "footprint": "0603", "at": [12, -6]},
    {"ref": "C1", "kind": "capacitor", "footprint": "0603", "at": [-18, 0]},
    {"ref": "C2", "kind": "capacitor", "footprint": "0603", "at": [18, 0]},
    {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [0, 12]}
  ],
  "nets": {
    "GND": ["J1.2", "U1.1", "U2.1", "C1.2", "C2.2"],
    "3V3": ["J1.1", "U1.8", "U2.8"],
    "N1": ["U1.2", "R1.1"],
    "N2": ["R1.2", "U2.2"],
    "N3": ["U1.3", "R2.1"],
    "N4": ["R2.2", "U2.3"],
    "N5": ["U1.4", "R3.1"],
    "N6": ["R3.2", "U2.4"],
    "N7": ["U1.5", "R4.1"],
    "N8": ["R4.2", "U2.5"],
    "N9": ["U1.6", "R5.1"],
    "N10": ["R5.2", "U2.6"],
    "N11": ["U1.7", "C1.1"],
    "N12": ["U2.7", "C2.1"]
  },
  "pours": [{"net": "GND", "layer": "inner1"}]
}"#;

/// Clean 2-layer board with parts spread far apart (x = -70 and x = +70 on a 160 mm board).
const DRC_LARGE_BOARD_JSON: &str = r#"{
  "board": {"width": 160, "height": 40, "layers": 2, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "parts": [
    {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [-70, 0]},
    {"ref": "R2", "kind": "resistor", "footprint": "0603", "at": [-65, 0]},
    {"ref": "R3", "kind": "resistor", "footprint": "0603", "at": [65, 0]},
    {"ref": "R4", "kind": "resistor", "footprint": "0603", "at": [70, 0]}
  ],
  "nets": {
    "LEFT": ["R1.1", "R2.1"],
    "LEFT2": ["R1.2", "R2.2"],
    "RIGHT": ["R3.1", "R4.1"],
    "RIGHT2": ["R3.2", "R4.2"]
  }
}"#;

/// 555 timer blinker reference board from pcb_native.rs.
const BLINKER_555_BOARD_JSON: &str = r#"{
  "board": {"width": 40, "height": 30, "layers": 2, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "parts": [
    {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [0, 0], "pins": {"1": "GND", "2": "TRIG", "3": "OUT", "4": "RESET", "5": "CTRL", "6": "THRES", "7": "DISCH", "8": "VCC"}},
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0603", "at": [-10, 5]},
    {"ref": "R2", "kind": "resistor", "value": "10k", "footprint": "0603", "at": [-10, -5]},
    {"ref": "R3", "kind": "resistor", "value": "330", "footprint": "0603", "at": [10, 5]},
    {"ref": "C1", "kind": "capacitor", "value": "10uF", "footprint": "0603", "at": [-10, -10]},
    {"ref": "C2", "kind": "capacitor", "value": "10nF", "footprint": "0603", "at": [10, -5]},
    {"ref": "LED1", "kind": "led", "value": "red", "footprint": "0603", "at": [15, 5]},
    {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [0, -10]}
  ],
  "nets": {
    "VCC": ["J1.1", "U1.8", "U1.4", "R1.1"],
    "GND": ["J1.2", "U1.1", "C1.2", "C2.2", "LED1.K"],
    "DISCH": ["R1.2", "R2.1", "U1.7"],
    "TIMING": ["R2.2", "C1.1", "U1.6", "U1.2"],
    "CTRL": ["U1.5", "C2.1"],
    "OUT": ["U1.3", "R3.1"],
    "LED_NET": ["R3.2", "LED1.A"]
  }
}"#;

/// Connector board with USB-C, barrel jack, JST-PH reference board from pcb_native.rs.
const CONNECTOR_BOARD_JSON: &str = r#"{
  "board": { "width": 60, "height": 40, "layers": 2, "thickness": 1.6 },
  "rules": { "trace": 0.25, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3 },
  "parts": [
    { "ref": "J1", "kind": "header", "footprint": "usb_c_16", "at": [-15, 0] },
    { "ref": "J2", "kind": "header", "footprint": "barrel_jack", "at": [15, -4] },
    { "ref": "J3", "kind": "header", "footprint": "jst_ph_2", "at": [0, 10] },
    { "ref": "D1", "kind": "led", "footprint": "0603", "at": [0, -10] },
    { "ref": "R1", "kind": "resistor", "footprint": "0603", "at": [0, 0] }
  ],
  "nets": {
    "VBUS": ["J1.A4", "J1.B9", "J2.1", "J3.2", "R1.1"],
    "LED_ANODE": ["R1.2", "D1.A"],
    "GND": ["D1.K", "J1.A1", "J1.B12", "J1.S1", "J2.2", "J3.1"]
  }
}"#;

fn qfn_board_spec(via_in_pad: bool) -> String {
    json!({
        "board": {"width": 30, "height": 30, "layers": 4, "thickness": 1.6},
        "rules": {"trace": 0.15, "clearance": 0.15, "via": [0.5, 0.25], "edge": 0.3, "via_in_pad": via_in_pad},
        "parts": [
            {"ref": "U1", "kind": "chip", "footprint": "qfn32", "at": [0, 0]},
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [0, 10]}
        ],
        "nets": {
            "GND": ["U1.33", "J1.1"],
            "SIG": ["U1.1", "J1.2"]
        },
        "pours": [{"net": "GND", "layer": "inner1"}]
    })
    .to_string()
}

// =============================================================================
// CONTRACT A: Plane layers (4-layer boards)
// =============================================================================

#[tokio::test]
async fn contract_a1_to_a5_plane_layers() {
    let dir = project(
        "contract_a",
        &[("pcb/plane.pcb.json", FOUR_LAYER_PLANE_BOARD_JSON)],
    );

    // A.4: pcb_build succeeds and pcb_drc reports "No DRC errors."
    let build_out = run(
        "pcb_build",
        json!({"source": "pcb/plane.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(
        build_out
            .text
            .contains("No errors: every connection is routed."),
        "Contract A.4 violation: pcb_build failed: {}",
        build_out.text
    );

    let drc_out = run("pcb_drc", json!({"board": "plane"}), &dir.path)
        .await
        .unwrap();
    assert!(
        drc_out.text.ends_with("No DRC errors."),
        "Contract A.4 violation: pcb_drc failed: {}",
        drc_out.text
    );

    let circuit_raw = std::fs::read_to_string(dir.path.join("pcb/plane.circuit.json")).unwrap();
    let circuit: Value = serde_json::from_str(&circuit_raw).unwrap();
    let elements = circuit.as_array().unwrap();

    // A.1: No pcb_trace segment of ANY net lies on a plane layer (inner1).
    let traces: Vec<_> = elements
        .iter()
        .filter(|e| e["type"] == "pcb_trace")
        .collect();
    assert!(!traces.is_empty(), "circuit must contain traces");
    for trace in &traces {
        let route = trace["route"].as_array().unwrap();
        for point in route {
            let layer = point["layer"].as_str().unwrap();
            assert_ne!(
                layer, "inner1",
                "Contract A.1 violation: trace {} of net {:?} lies on plane layer inner1",
                trace["pcb_trace_id"], trace["net"]
            );
        }
    }

    // A.2 & A.3:
    // Plane-net pads are NOT joined by trace trees.
    // Three SMD GND pads spread apart (C1.2, C2.2, C3.2):
    // there is no pcb_trace whose net is GND that runs between two different GND pads;
    // count GND vias >= number of GND SMD pads.
    // A through-hole pad on the plane net (J1.1) needs no extra via.
    let gnd_vias: Vec<_> = elements
        .iter()
        .filter(|e| e["type"] == "pcb_via" && e["net"] == "GND")
        .collect();
    assert!(
        gnd_vias.len() >= 3,
        "Contract A.2 violation: expected at least 3 GND vias for 3 SMD GND pads, got {}",
        gnd_vias.len()
    );
    assert_eq!(
        gnd_vias.len(),
        3,
        "Contract A.3 violation: through-hole pad J1.1 should need no extra via; expected exactly 3 GND vias, got {}",
        gnd_vias.len()
    );

    let smd_gnd_pads: Vec<_> = elements
        .iter()
        .filter(|e| e["type"] == "pcb_smtpad" && e["net"] == "GND")
        .map(|p| (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap()))
        .collect();
    assert_eq!(smd_gnd_pads.len(), 3, "expected 3 SMD GND pads");

    let th_gnd_pads: Vec<_> = elements
        .iter()
        .filter(|e| e["type"] == "pcb_plated_hole" && e["net"] == "GND")
        .map(|p| (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap()))
        .collect();
    assert_eq!(
        th_gnd_pads.len(),
        1,
        "expected 1 through-hole GND pad (J1.1)"
    );

    let mut all_gnd_pads = smd_gnd_pads.clone();
    all_gnd_pads.extend(th_gnd_pads);

    let gnd_traces: Vec<_> = traces.iter().filter(|t| t["net"] == "GND").collect();
    for trace in gnd_traces {
        let route = trace["route"].as_array().unwrap();
        let p0 = (
            route[0]["x"].as_f64().unwrap(),
            route[0]["y"].as_f64().unwrap(),
        );
        let p1 = (
            route[1]["x"].as_f64().unwrap(),
            route[1]["y"].as_f64().unwrap(),
        );

        let find_pad = |pt: (f64, f64)| -> Option<usize> {
            all_gnd_pads
                .iter()
                .position(|pad| ((pad.0 - pt.0).powi(2) + (pad.1 - pt.1).powi(2)).sqrt() < 0.1)
        };
        let pad0 = find_pad(p0);
        let pad1 = find_pad(p1);
        if let (Some(i0), Some(i1)) = (pad0, pad1) {
            assert_eq!(
                i0, i1,
                "Contract A.2 violation: GND trace connects two distinct GND pads: {:?} and {:?}",
                all_gnd_pads[i0], all_gnd_pads[i1]
            );
        }
    }

    // A.5: The plane Gerber (-In1_Cu.gbr for inner1) contains copper (G36 regions)
    // and, when there are vias of other nets, uses clear polarity %LPC*% for antipads/holes
    // and returns to %LPD*% afterwards (each %LPC*% is later followed by %LPD*%).
    let in1_gbr = std::fs::read_to_string(dir.path.join("fab/plane/plane-In1_Cu.gbr")).unwrap();
    assert!(
        in1_gbr.contains("G36*"),
        "Contract A.5 violation: inner1 Gerber must contain G36 copper regions"
    );
    assert!(
        in1_gbr.contains("%LPC*%"),
        "Contract A.5 violation: inner1 Gerber must contain %LPC*% clear polarity for antipads"
    );

    let mut lpc_indices = Vec::new();
    let mut lpd_indices = Vec::new();
    for (i, line) in in1_gbr.lines().enumerate() {
        if line == "%LPC*%" {
            lpc_indices.push(i);
        } else if line == "%LPD*%" {
            lpd_indices.push(i);
        }
    }
    assert!(
        !lpc_indices.is_empty(),
        "must have at least one %LPC*% antipad region"
    );
    for &lpc_idx in &lpc_indices {
        let has_subsequent_lpd = lpd_indices.iter().any(|&lpd_idx| lpd_idx > lpc_idx);
        assert!(
            has_subsequent_lpd,
            "Contract A.5 violation: %LPC*% at line {} is not followed by %LPD*%",
            lpc_idx
        );
    }
}

#[tokio::test]
async fn contract_a6_qfn_exposed_pad_via_in_pad() {
    // With via_in_pad: true, a QFN exposed pad (pad 33) on GND is connected to the inner GND plane
    // and DRC is clean.
    let dir_true = project("qfn_true", &[("pcb/qfn.pcb.json", &qfn_board_spec(true))]);
    let build_true = run(
        "pcb_build",
        json!({"source": "pcb/qfn.pcb.json"}),
        &dir_true.path,
    )
    .await
    .unwrap();
    assert!(
        build_true
            .text
            .contains("No errors: every connection is routed."),
        "Contract A.6: expected clean build with via_in_pad: true, got: {}",
        build_true.text
    );
    let drc_true = run("pcb_drc", json!({"board": "qfn"}), &dir_true.path)
        .await
        .unwrap();
    assert!(
        drc_true.text.ends_with("No DRC errors."),
        "Contract A.6: expected clean DRC with via_in_pad: true, got: {}",
        drc_true.text
    );

    // Verify circuit has via for pad 33
    let circuit_raw = std::fs::read_to_string(dir_true.path.join("pcb/qfn.circuit.json")).unwrap();
    let circuit: Value = serde_json::from_str(&circuit_raw).unwrap();
    let gnd_vias = circuit
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "pcb_via" && e["net"] == "GND")
        .count();
    assert!(
        gnd_vias >= 1,
        "Contract A.6: expected GND via connected to QFN pad 33"
    );

    // With via_in_pad: false, the same board fails to route GND or reports it.
    let dir_false = project("qfn_false", &[("pcb/qfn.pcb.json", &qfn_board_spec(false))]);
    let build_false = run(
        "pcb_build",
        json!({"source": "pcb/qfn.pcb.json"}),
        &dir_false.path,
    )
    .await;
    match build_false {
        Err(err) => {
            let err_text = err.to_string();
            assert!(
                err_text.contains("unroutable: net GND"),
                "Contract A.6: expected error reporting unroutable GND net, got: {err_text}"
            );
        }
        Ok(out) => {
            // If build didn't error, verify it reported unroutable or failed DRC
            assert!(
                !out.text.contains("No errors: every connection is routed.")
                    || out.text.contains("unroutable: net GND"),
                "Contract A.6: via_in_pad: false should not cleanly route without errors"
            );
        }
    }
}

#[tokio::test]
async fn contract_a7_outer_layer_pour_retains_traces() {
    // A pour on an OUTER layer (top/bottom) keeps old behaviour:
    // GND pads may be joined by traces; build + DRC clean.
    let dir = project(
        "outer_pour",
        &[("pcb/outer.pcb.json", OUTER_LAYER_POUR_BOARD_JSON)],
    );
    let build_out = run(
        "pcb_build",
        json!({"source": "pcb/outer.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(
        build_out
            .text
            .contains("No errors: every connection is routed."),
        "Contract A.7: outer pour board build failed: {}",
        build_out.text
    );

    let drc_out = run("pcb_drc", json!({"board": "outer"}), &dir.path)
        .await
        .unwrap();
    assert!(
        drc_out.text.ends_with("No DRC errors."),
        "Contract A.7: outer pour board DRC failed: {}",
        drc_out.text
    );

    let circuit_raw = std::fs::read_to_string(dir.path.join("pcb/outer.circuit.json")).unwrap();
    let circuit: Value = serde_json::from_str(&circuit_raw).unwrap();
    let traces = circuit
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "pcb_trace" && e["net"] == "GND")
        .count();
    assert!(
        traces >= 1,
        "Contract A.7: outer layer pour should join pads with traces"
    );
}

// =============================================================================
// CONTRACT B: Grid limit
// =============================================================================

#[tokio::test]
async fn contract_b_grid_limit_200mm_board_accepted() {
    let spec = json!({
        "board": {"width": 200, "height": 200, "layers": 4},
        "rules": {"clearance": 0.1}
    })
    .to_string();
    let dir = project("grid_200", &[("pcb/grid200.pcb.json", &spec)]);
    let res = run(
        "pcb_build",
        json!({"source": "pcb/grid200.pcb.json"}),
        &dir.path,
    )
    .await;
    match res {
        Ok(out) => {
            assert!(
                !out.text.contains("board is too large for the routing grid"),
                "Contract B violation: 200 mm board must not fail with grid limit error: {}",
                out.text
            );
        }
        Err(err) => {
            let msg = err.to_string();
            assert!(
                !msg.contains("board is too large for the routing grid"),
                "Contract B violation: 200 mm board must not fail with grid limit error: {msg}"
            );
        }
    }
}

#[tokio::test]
async fn contract_b_grid_limit_400mm_board_rejected() {
    let spec = json!({
        "board": {"width": 400, "height": 400, "layers": 4},
        "rules": {"clearance": 0.1}
    })
    .to_string();
    let dir = project("grid_400", &[("pcb/grid400.pcb.json", &spec)]);
    let res = run(
        "pcb_build",
        json!({"source": "pcb/grid400.pcb.json"}),
        &dir.path,
    )
    .await;
    assert!(res.is_err(), "Contract B: 400 mm board must fail pcb_build");
    let err_msg = res.unwrap_err().to_string();
    assert!(
        err_msg.contains("board is too large for the routing grid"),
        "Contract B violation: error must contain exactly 'board is too large for the routing grid', got: {err_msg}"
    );
}

// =============================================================================
// CONTRACT C: Determinism
// =============================================================================

#[tokio::test]
async fn contract_c_determinism_multi_net_board() {
    let dir1 = project("det1", &[("pcb/det.pcb.json", DETERMINISM_BOARD_JSON)]);
    let build1 = run(
        "pcb_build",
        json!({"source": "pcb/det.pcb.json"}),
        &dir1.path,
    )
    .await
    .unwrap();
    assert!(build1
        .text
        .contains("No errors: every connection is routed."));

    let dir2 = project("det2", &[("pcb/det.pcb.json", DETERMINISM_BOARD_JSON)]);
    let build2 = run(
        "pcb_build",
        json!({"source": "pcb/det.pcb.json"}),
        &dir2.path,
    )
    .await
    .unwrap();
    assert!(build2
        .text
        .contains("No errors: every connection is routed."));

    let circuit1 = std::fs::read(dir1.path.join("pcb/det.circuit.json")).unwrap();
    let circuit2 = std::fs::read(dir2.path.join("pcb/det.circuit.json")).unwrap();
    assert_eq!(
        circuit1, circuit2,
        "Contract C violation: pcb/<name>.circuit.json must be byte-identical"
    );

    let gbr1 = std::fs::read_to_string(dir1.path.join("fab/det/det-F_Cu.gbr")).unwrap();
    let gbr2 = std::fs::read_to_string(dir2.path.join("fab/det/det-F_Cu.gbr")).unwrap();
    assert_eq!(
        gbr1, gbr2,
        "Contract C violation: F_Cu.gbr must be identical"
    );
}

// =============================================================================
// CONTRACT D: DRC spatial index correctness
// =============================================================================

#[tokio::test]
async fn contract_d_drc_spatial_index_catches_fault_on_big_coordinates() {
    let dir = project("drc_large", &[("pcb/large.pcb.json", DRC_LARGE_BOARD_JSON)]);
    let build_out = run(
        "pcb_build",
        json!({"source": "pcb/large.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(build_out
        .text
        .contains("No errors: every connection is routed."));

    let drc_initial = run("pcb_drc", json!({"board": "large"}), &dir.path)
        .await
        .unwrap();
    assert!(
        drc_initial.text.ends_with("No DRC errors."),
        "Contract D: initial DRC must be clean: {}",
        drc_initial.text
    );

    let circuit_path = dir.path.join("pcb/large.circuit.json");
    let original_circuit_raw = std::fs::read_to_string(&circuit_path).unwrap();
    let mut circuit: Value = serde_json::from_str(&original_circuit_raw).unwrap();

    // Move one route point of net LEFT to x = 70.0 (near R4 on net RIGHT, 140 mm away)
    let trace = circuit
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["type"] == "pcb_trace" && e["net"] == "LEFT")
        .expect("must find a trace on net LEFT");
    let route = trace["route"].as_array_mut().unwrap();
    route[0]["x"] = json!(70.0);
    route[0]["y"] = json!(0.0);

    std::fs::write(&circuit_path, circuit.to_string()).unwrap();

    let drc_faulted = run("pcb_drc", json!({"board": "large"}), &dir.path)
        .await
        .unwrap();
    assert!(
        drc_faulted.text.contains("clearance: "),
        "Contract D violation: DRC report must contain a clearance fault: {}",
        drc_faulted.text
    );
    let last_line = drc_faulted.text.lines().last().unwrap_or("");
    assert!(
        last_line.ends_with("DRC error(s).") && !last_line.starts_with("0"),
        "Contract D violation: DRC report must end with 'N DRC error(s).' with N >= 1, got: {last_line}"
    );

    // Second edit: move it back, verify clean DRC
    std::fs::write(&circuit_path, &original_circuit_raw).unwrap();

    let drc_restored = run("pcb_drc", json!({"board": "large"}), &dir.path)
        .await
        .unwrap();
    assert!(
        drc_restored.text.ends_with("No DRC errors."),
        "Contract D violation: restored DRC must have no errors: {}",
        drc_restored.text
    );
}

// =============================================================================
// CONTRACT E: Regression
// =============================================================================

#[tokio::test]
async fn contract_e_regression_555_blinker() {
    let dir = project(
        "reg_555",
        &[("pcb/blinker.pcb.json", BLINKER_555_BOARD_JSON)],
    );
    let build_out = run(
        "pcb_build",
        json!({"source": "pcb/blinker.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(
        build_out
            .text
            .contains("No errors: every connection is routed."),
        "Contract E: 555 blinker build failed: {}",
        build_out.text
    );

    let drc_out = run("pcb_drc", json!({"board": "blinker"}), &dir.path)
        .await
        .unwrap();
    assert!(
        drc_out.text.ends_with("No DRC errors."),
        "Contract E: 555 blinker DRC failed: {}",
        drc_out.text
    );
}

#[tokio::test]
async fn contract_e_regression_connector_board() {
    let dir = project("reg_conn", &[("pcb/conn.pcb.json", CONNECTOR_BOARD_JSON)]);
    let build_out = run(
        "pcb_build",
        json!({"source": "pcb/conn.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(
        build_out
            .text
            .contains("No errors: every connection is routed."),
        "Contract E: connector board build failed: {}",
        build_out.text
    );

    let drc_out = run("pcb_drc", json!({"board": "conn"}), &dir.path)
        .await
        .unwrap();
    assert!(
        drc_out.text.ends_with("No DRC errors."),
        "Contract E: connector board DRC failed: {}",
        drc_out.text
    );
}
