mod common;
#[path = "common/tools.rs"]
mod support;

use serde_json::{json, Value};
use std::path::Path;
use support::{project, run};

// Reference board definitions per plan.md section 3

const LED_BOARD_JSON: &str = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "netclasses": {"power": {"nets": ["VCC", "GND"], "trace": 0.4}},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [-6, 0]},
    {"ref": "LED1", "kind": "led", "value": "red", "footprint": "0603", "at": [6, 0], "rotation": 90},
    {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [0, -6]},
    {"ref": "U1", "kind": "chip", "footprint": "soic8", "pins": {"1": "VCC", "8": "GND"}, "side": "bottom", "lcsc": "C123"}
  ],
  "nets": {"IN": ["J1.1", "R1.1"], "A": ["R1.2", "LED1.A"], "GND": ["LED1.K", "J1.2"]},
  "holes": [{"at": [12, 7], "d": 3.2}],
  "pours": [{"net": "GND", "layer": "bottom"}],
  "keepouts": [{"rect": [-3, 6, 24, 6], "layers": ["bottom"]}],
  "traces": [{"net": "RF", "width": 1.1}]
}"#;

const ANTENNA_BOARD_JSON: &str = r#"{
  "board": {"width": 50, "height": 40, "layers": 2, "thickness": 1.6},
  "parts": [
    {"ref": "ANT1", "kind": "antenna", "shape": "2.4ghz_quarter_wave_monopole", "at": [-20, 18.4]}
  ],
  "pours": [{"net": "GND", "layer": "bottom"}],
  "keepouts": [{"rect": [-3, 16, 32, 8], "layers": ["bottom"]}]
}"#;

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

const FOUR_LAYER_BOARD_JSON: &str = r#"{
  "board": {"width": 40, "height": 30, "layers": 4, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [-10, 0]},
    {"ref": "R2", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [10, 0]}
  ],
  "nets": {
    "SIG": ["R1.1", "R2.1"],
    "GND": ["R1.2", "R2.2"]
  }
}"#;

const AUTO_PLACED_BOARD_JSON: &str = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402"},
    {"ref": "LED1", "kind": "led", "value": "red", "footprint": "0603"},
    {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2"}
  ],
  "nets": {
    "IN": ["J1.1", "R1.1"],
    "A": ["R1.2", "LED1.A"],
    "GND": ["LED1.K", "J1.2"]
  }
}"#;

const TWO_RESISTOR_BOARD_JSON: &str = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [-5, 0]},
    {"ref": "R2", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [5, 0]}
  ],
  "nets": {
    "A": ["R1.1", "R2.1"],
    "B": ["R1.2", "R2.2"]
  }
}"#;

fn assert_gerber_syntax(path: &Path) {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read Gerber {}: {e}", path.display()));
    assert!(
        content.contains("%FSLA"),
        "missing %FSLA in {}",
        path.display()
    );
    assert!(
        content.contains("%MO"),
        "missing %MO in {}",
        path.display()
    );
    assert!(
        content.contains("M02*"),
        "missing M02* EOF in {}",
        path.display()
    );
}

fn assert_excellon_syntax(path: &Path) {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read Excellon {}: {e}", path.display()));
    assert!(
        content.contains("METRIC") || content.contains("M48") || content.contains("M71"),
        "missing METRIC/M48 header in {}",
        path.display()
    );
    assert!(
        content.contains("M30"),
        "missing M30 EOF in {}",
        path.display()
    );
}

fn assert_js_formatted_json(path: &Path) {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read JSON {}: {e}", path.display()));
    let _parsed: Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("invalid JSON in {}: {e}", path.display()));
    for line in raw.lines() {
        for token in line.split([',', ':', '[', ']', '{', '}']) {
            let t = token.trim();
            if t.parse::<f64>().is_ok() {
                assert_ne!(t, "-0", "Found -0 in JSON: {line}");
                assert_ne!(t, "-0.0", "Found -0.0 in JSON: {line}");
                if t.contains('.') {
                    assert!(!t.ends_with(".0"), "Found redundant .0 in number '{t}': {line}");
                }
            }
        }
    }
}

/// Extracts all numbers immediately preceding the given unit (case-insensitive) in text.
/// Matches patterns like "0.30 mm", "0.300mm", "1.53 mΩ", "5.08 A".
fn parse_numbers_with_unit(text: &str, unit: &str) -> Vec<f64> {
    let mut numbers = Vec::new();
    let lower_text = text.to_lowercase();
    let lower_unit = unit.to_lowercase();
    let mut search_start = 0;
    while let Some(idx) = lower_text[search_start..].find(&lower_unit) {
        let unit_idx = search_start + idx;
        search_start = unit_idx + lower_unit.len();

        // Ensure the match is not a prefix of a longer alphabetic word
        if let Some(next_char) = text[unit_idx + lower_unit.len()..].chars().next() {
            if next_char.is_alphabetic() && next_char != 'Ω' && next_char != 'ω' {
                continue;
            }
        }

        // Look backwards from unit_idx for a number
        let prefix = &text[..unit_idx];
        let trimmed = prefix.trim_end();
        let mut token_start = trimmed.len();
        for (i, c) in trimmed.char_indices().rev() {
            if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' {
                token_start = i;
            } else {
                break;
            }
        }
        if token_start < trimmed.len() {
            if let Ok(num) = trimmed[token_start..].parse::<f64>() {
                numbers.push(num);
            }
        }
    }
    numbers
}

// -----------------------------------------------------------------------------
// Step 2 Footprints (Reference bullets in 3.2)
// -----------------------------------------------------------------------------

#[cfg(any())] // step 2 removes this line
mod fp {
    use forge_server::tools::pcb::footprint;

    #[test]
    #[ignore = "stage 3 not built yet"]
    fn fp_0402() {
        let fp = footprint("0402").expect("footprint 0402 should be generated");
        assert_eq!(fp.pads.len(), 2);
        let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
        let p2 = fp.pads.iter().find(|p| p.number == "2").unwrap();
        // 0402 nominal pad: 0.59 x 0.64 mm, opposite-pad centres: 0.97 mm
        assert!((p1.size[0] - 0.59).abs() < 1e-4, "pad size x {} != 0.59", p1.size[0]);
        assert!((p1.size[1] - 0.64).abs() < 1e-4, "pad size y {} != 0.64", p1.size[1]);
        let centres = (p2.at[0] - p1.at[0]).abs();
        assert!((centres - 0.97).abs() < 1e-4, "centres {} != 0.97", centres);
    }

    #[test]
    #[ignore = "stage 3 not built yet"]
    fn fp_0603() {
        let fp = footprint("0603").expect("footprint 0603 should be generated");
        assert_eq!(fp.pads.len(), 2);
        let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
        let p2 = fp.pads.iter().find(|p| p.number == "2").unwrap();
        // 0603 nominal pad: 0.875 x 0.95 mm, opposite-pad centres: 1.575 mm
        assert!((p1.size[0] - 0.875).abs() < 1e-4, "pad size x {} != 0.875", p1.size[0]);
        assert!((p1.size[1] - 0.95).abs() < 1e-4, "pad size y {} != 0.95", p1.size[1]);
        let centres = (p2.at[0] - p1.at[0]).abs();
        assert!((centres - 1.575).abs() < 1e-4, "centres {} != 1.575", centres);
    }

    #[test]
    #[ignore = "stage 3 not built yet"]
    fn fp_soic8() {
        let fp = footprint("soic8").expect("footprint soic8 should be generated");
        assert_eq!(fp.pads.len(), 8);
        let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
        let p8 = fp.pads.iter().find(|p| p.number == "8").unwrap();
        // soic8 nominal pad: 1.95 x 0.60 mm, opposite-pad centres: 4.95 mm
        assert!((p1.size[0] - 1.95).abs() < 1e-4, "pad size x {} != 1.95", p1.size[0]);
        assert!((p1.size[1] - 0.60).abs() < 1e-4, "pad size y {} != 0.60", p1.size[1]);
        let centres = (p8.at[0] - p1.at[0]).abs();
        assert!((centres - 4.95).abs() < 1e-4, "centres {} != 4.95", centres);
    }

    #[test]
    #[ignore = "stage 3 not built yet"]
    fn fp_qfn32() {
        let fp = footprint("qfn32").expect("footprint qfn32 should be generated");
        assert_eq!(fp.pads.len(), 33);
        let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
        let p24 = fp.pads.iter().find(|p| p.number == "24").unwrap();
        // qfn32 nominal perimeter pad: 0.875 x 0.25 mm, opposite-pad centres: 4.875 mm
        assert!((p1.size[0] - 0.875).abs() < 1e-4, "pad size x {} != 0.875", p1.size[0]);
        assert!((p1.size[1] - 0.25).abs() < 1e-4, "pad size y {} != 0.25", p1.size[1]);
        let centres = (p24.at[0] - p1.at[0]).abs();
        assert!((centres - 4.875).abs() < 1e-4, "centres {} != 4.875", centres);
        // Exposed pad (EP): 3.1 x 3.1 mm
        let ep = fp.pads.iter().find(|p| p.number == "33" || p.number == "EP").unwrap();
        assert!((ep.size[0] - 3.1).abs() < 1e-4, "EP size x {} != 3.1", ep.size[0]);
        assert!((ep.size[1] - 3.1).abs() < 1e-4, "EP size y {} != 3.1", ep.size[1]);
    }
}

// -----------------------------------------------------------------------------
// Step 2 Spec Validation Errors (Section 3.5)
// -----------------------------------------------------------------------------

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn spec_error_source_must_be_pcb_json() {
    let dir = project("spec_err1", &[("pcb/invalid.txt", "{}")]);
    let err = run("pcb_build", json!({"source": "pcb/invalid.txt"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("source must be a .pcb.json file"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn spec_error_source_does_not_exist() {
    let dir = project("spec_err2", &[] as &[(&str, &str)]);
    let err = run("pcb_build", json!({"source": "pcb/missing.pcb.json"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("pcb/missing.pcb.json does not exist; write it with write_file first"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn spec_error_invalid_name() {
    let dir = project("spec_err3", &[("pcb/bad space.pcb.json", "{}")]);
    let err = run("pcb_build", json!({"source": "pcb/bad space.pcb.json"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("name may only use letters, digits, - and _"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn spec_error_unknown_pin() {
    let bad_board = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [0, 0]}
  ],
  "nets": {"IN": ["R1.3"]}
}"#;
    let dir = project("spec_err4", &[("pcb/bad_pin.pcb.json", bad_board)]);
    let err = run("pcb_build", json!({"source": "pcb/bad_pin.pcb.json"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("R1.3: R1 (0402) has pins 1, 2"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn spec_error_unknown_footprint() {
    let bad_board = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "nonexistent_fp_xyz", "at": [0, 0]}
  ],
  "nets": {}
}"#;
    let dir = project("spec_err5", &[("pcb/bad_fp.pcb.json", bad_board)]);
    let err = run("pcb_build", json!({"source": "pcb/bad_fp.pcb.json"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("unknown footprint nonexistent_fp_xyz; use one of:"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn spec_error_part_off_board() {
    let bad_board = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [100, 100]}
  ],
  "nets": {}
}"#;
    let dir = project("spec_err6", &[("pcb/off_board.pcb.json", bad_board)]);
    let err = run("pcb_build", json!({"source": "pcb/off_board.pcb.json"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("R1 is off the board"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn spec_error_parts_overlap() {
    let bad_board = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [0, 0]},
    {"ref": "R2", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [0, 0]}
  ],
  "nets": {}
}"#;
    let dir = project("spec_err7", &[("pcb/overlap.pcb.json", bad_board)]);
    let err = run("pcb_build", json!({"source": "pcb/overlap.pcb.json"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("parts overlap: R1 and R2"));
}

// -----------------------------------------------------------------------------
// Step 3 Router & Pours (Section 3.4 & unroutable error in 3.5)
// -----------------------------------------------------------------------------

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn route_error_unroutable() {
    let unroutable_board = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [-10, 0]},
    {"ref": "R2", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [10, 0]}
  ],
  "keepouts": [
    {"rect": [0, 0, 5, 20], "layers": ["top", "bottom"]}
  ],
  "nets": {"SIG": ["R1.1", "R2.1"]}
}"#;
    let dir = project("route_unroutable", &[("pcb/unroutable.pcb.json", unroutable_board)]);
    let err = run("pcb_build", json!({"source": "pcb/unroutable.pcb.json"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("unroutable: net SIG (R1.1 to R2.1)"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn route_led_board() {
    let dir = project("route_led", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("No errors: every connection is routed."));
    assert!(out.text.contains("board 30 × 20 mm, 1.6 mm thick, 2 copper layers."));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn route_555_blinker_no_errors() {
    let dir = project("route_555", &[("pcb/blinker.pcb.json", BLINKER_555_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/blinker.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("No errors: every connection is routed."));
    assert!(!out.text.contains("ERROR"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn route_4layer_board() {
    let dir = project("route_4l", &[("pcb/four.pcb.json", FOUR_LAYER_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/four.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("4 copper layers."));
    assert!(out.text.contains("No errors: every connection is routed."));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn route_auto_placed_board_no_at() {
    let dir = project("route_autoplace", &[("pcb/auto.pcb.json", AUTO_PLACED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/auto.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("No errors: every connection is routed."));
    let board_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path.join("pcb/auto.board.json")).unwrap(),
    )
    .unwrap();
    let components = board_json["components"].as_array().expect("components array");
    assert_eq!(components.len(), 3);
    for comp in components {
        let min_x = comp["min"][0].as_f64().unwrap();
        let max_x = comp["max"][0].as_f64().unwrap();
        let min_y = comp["min"][1].as_f64().unwrap();
        let max_y = comp["max"][1].as_f64().unwrap();
        assert!(min_x >= -15.0 && max_x <= 15.0, "component outside x bounds: {min_x}..{max_x}");
        assert!(min_y >= -10.0 && max_y <= 10.0, "component outside y bounds: {min_y}..{max_y}");
    }
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn pour_ground_plane_generates_g36() {
    let dir = project("pour_gnd", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("Copper pours: GND on bottom"));
    let b_cu = std::fs::read_to_string(dir.path.join("fab/led/led-B_Cu.gbr")).unwrap();
    assert!(b_cu.contains("G36"), "bottom copper Gerber must contain G36 region code for pour");
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn pour_antenna_clears_ground_under_keepout() {
    let dir = project("pour_ant", &[("pcb/ant.pcb.json", ANTENNA_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/ant.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("No errors"));
    let circuit: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path.join("pcb/ant.circuit.json")).unwrap(),
    )
    .unwrap();
    let pour = circuit
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "pcb_copper_pour")
        .expect("circuit.json must contain pcb_copper_pour");
    let max_y = pour["brep_shape"]["outer_ring"]["vertices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["x"].as_f64().unwrap() > -15. && v["x"].as_f64().unwrap() < 10.)
        .map(|v| v["y"].as_f64().unwrap())
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(max_y <= 12.1, "ground reaches y {max_y} under the antenna (must be <= 12.1)");
}

// -----------------------------------------------------------------------------
// Step 4 Outputs & Reports (Section 3.3)
// -----------------------------------------------------------------------------

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_pcb_svg() {
    let dir = project("out_pcb_svg", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.files.contains(&"pcb/led-pcb.svg".into()));
    let content = std::fs::read_to_string(dir.path.join("pcb/led-pcb.svg")).unwrap();
    assert!(content.contains("<svg") && content.contains("</svg>"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_schematic_svg() {
    let dir = project("out_sch_svg", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.files.contains(&"pcb/led-schematic.svg".into()));
    let content = std::fs::read_to_string(dir.path.join("pcb/led-schematic.svg")).unwrap();
    assert!(content.contains("<svg") && content.contains("</svg>"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_glb() {
    let dir = project("out_glb", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.files.contains(&"pcb/led.glb".into()));
    let bytes = std::fs::read(dir.path.join("pcb/led.glb")).unwrap();
    assert!(bytes.starts_with(b"glTF"), "GLB must start with glTF binary magic");
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_step() {
    let dir = project("out_step", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.files.contains(&"pcb/led.step".into()));
    let content = std::fs::read_to_string(dir.path.join("pcb/led.step")).unwrap();
    assert!(content.contains("ISO-10303-21"), "STEP must contain ISO-10303-21 header");
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_board_json() {
    let dir = project("out_board_json", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.files.contains(&"pcb/led.board.json".into()));
    let p = dir.path.join("pcb/led.board.json");
    assert_js_formatted_json(&p);
    let val: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    assert!(val.get("size").is_some());
    assert!(val.get("components").is_some());
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_circuit_json() {
    let dir = project("out_circuit_json", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.files.contains(&"pcb/led.circuit.json".into()));
    let p = dir.path.join("pcb/led.circuit.json");
    assert_js_formatted_json(&p);
    let val: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let elems = val.as_array().expect("array of circuit elements");
    assert!(elems.iter().any(|e| e["type"] == "pcb_board"));
    assert!(elems.iter().any(|e| e["type"] == "pcb_trace"));
    assert!(elems.iter().any(|e| e["type"] == "pcb_copper_pour"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_copper_f_cu_gerber() {
    let dir = project("out_f_cu", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-F_Cu.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_copper_b_cu_gerber() {
    let dir = project("out_b_cu", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-B_Cu.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_copper_in1_cu_4layer_gerber() {
    let dir = project("out_in1", &[("pcb/four.pcb.json", FOUR_LAYER_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/four.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/four/four-In1_Cu.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_copper_in2_cu_4layer_gerber() {
    let dir = project("out_in2", &[("pcb/four.pcb.json", FOUR_LAYER_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/four.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/four/four-In2_Cu.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_mask_f_mask_gerber() {
    let dir = project("out_f_mask", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-F_Mask.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_mask_b_mask_gerber() {
    let dir = project("out_b_mask", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-B_Mask.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_paste_f_paste_gerber() {
    let dir = project("out_f_paste", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-F_Paste.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_paste_b_paste_gerber() {
    let dir = project("out_b_paste", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-B_Paste.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_silkscreen_f_silkscreen_gerber() {
    let dir = project("out_f_silk", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-F_Silkscreen.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_silkscreen_b_silkscreen_gerber() {
    let dir = project("out_b_silk", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-B_Silkscreen.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_edge_cuts_gerber() {
    let dir = project("out_edge", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-Edge_Cuts.gbr"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_drill_pth_excellon() {
    let dir = project("out_pth", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_excellon_syntax(&dir.path.join("fab/led/led-PTH.drl"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_drill_npth_excellon() {
    let dir = project("out_npth", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert_excellon_syntax(&dir.path.join("fab/led/led-NPTH.drl"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_pnp_csv() {
    let dir = project("out_pnp", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    let content = std::fs::read_to_string(dir.path.join("fab/led/led-pnp.csv")).unwrap();
    assert!(content.starts_with("Designator,Mid X,Mid Y,Layer,Rotation"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_file_fab_bom_csv() {
    let dir = project("out_bom", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    let content = std::fs::read_to_string(dir.path.join("fab/led/led-bom.csv")).unwrap();
    assert!(content.starts_with("Comment,Designator,Footprint,LCSC Part #"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_report_line_built_board() {
    let dir = project("out_rep_board", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("Built pcb/led.pcb.json: board 30 × 20 mm, 1.6 mm thick, 2 copper layers."));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_report_line_parts() {
    let dir = project("out_rep_parts", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("4 part(s):"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_report_line_routed_traces() {
    let dir = project("out_rep_traces", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("routed as") && out.text.contains("trace(s) with") && out.text.contains("via(s);") && out.text.contains("hole(s)."));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_report_line_copper_pours() {
    let dir = project("out_rep_pours", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("Copper pours: GND on bottom"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_report_line_no_errors() {
    let dir = project("out_rep_no_err", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("No errors: every connection is routed."));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn out_report_line_errors_summary() {
    let unroutable_board = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [-10, 0]},
    {"ref": "R2", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [10, 0]}
  ],
  "keepouts": [
    {"rect": [0, 0, 5, 20], "layers": ["top", "bottom"]}
  ],
  "nets": {"SIG": ["R1.1", "R2.1"]}
}"#;
    let dir = project("out_rep_errs", &[("pcb/err.pcb.json", unroutable_board)]);
    let err = run("pcb_build", json!({"source": "pcb/err.pcb.json"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("1 ERROR(S), fix them and rebuild:"));
}

// -----------------------------------------------------------------------------
// Step 5 DRC (Section 2 & 3.5)
// -----------------------------------------------------------------------------

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn drc_error_board_not_built() {
    let dir = project("drc_unbuilt", &[] as &[(&str, &str)]);
    let err = run("pcb_drc", json!({"board": "unbuilt"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("board unbuilt is not built; run pcb_build first"));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn drc_clean_board() {
    let dir = project("drc_clean", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/led.pcb.json"}), &dir.path)
        .await
        .unwrap();
    let out = run("pcb_drc", json!({"board": "led"}), &dir.path)
        .await
        .unwrap();
    let drc_path = dir.path.join("fab/led/led-drc.txt");
    assert!(drc_path.exists(), "fab/led/led-drc.txt must exist");
    let rpt = std::fs::read_to_string(&drc_path).unwrap();
    let lines: Vec<&str> = rpt.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.last().copied(), Some("No DRC errors."));
    assert!(out.text.contains("No DRC errors."));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn drc_broken_board_reports_violations() {
    let dir = project("drc_broken", &[("pcb/broken.pcb.json", TWO_RESISTOR_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/broken.pcb.json"}), &dir.path)
        .await
        .unwrap();

    let circuit_file = dir.path.join("pcb/broken.circuit.json");
    let content = std::fs::read_to_string(&circuit_file).unwrap();
    let mut circuit: Value = serde_json::from_str(&content).unwrap();

    let mut modified = false;
    for elem in circuit.as_array_mut().unwrap() {
        if elem["type"] == "pcb_smtpad" {
            if let Some(x) = elem.get("x").and_then(|v| v.as_f64()) {
                if x < -5.0 {
                    // R1.1 is at ~ -5.485 mm. Move it +0.28 mm towards R1.2 (at ~ -4.515 mm),
                    // reducing clearance from 0.38 mm to 0.10 mm (< 0.2 mm rule).
                    elem["x"] = json!(x + 0.28);
                    modified = true;
                    break;
                }
            }
        }
    }
    assert!(modified, "must have found and moved a pad");
    std::fs::write(&circuit_file, serde_json::to_string_pretty(&circuit).unwrap()).unwrap();

    let out = run("pcb_drc", json!({"board": "broken"}), &dir.path)
        .await
        .unwrap();
    let rpt = std::fs::read_to_string(dir.path.join("fab/broken/broken-drc.txt")).unwrap();
    let lines: Vec<&str> = rpt.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    assert!(lines.iter().any(|l| l.starts_with("clearance: ")));
    // The moved pad may also crowd the routed tracks, so only require at least one error.
    let last = lines.last().copied().unwrap_or("");
    assert!(last.ends_with(" DRC error(s).") && !last.starts_with("0 "), "{rpt}");
    assert!(out.text.contains("clearance: "));
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn drc_track_width_violation() {
    let dir = project("drc_width", &[("pcb/width.pcb.json", TWO_RESISTOR_BOARD_JSON)]);
    run("pcb_build", json!({"source": "pcb/width.pcb.json"}), &dir.path)
        .await
        .unwrap();

    let circuit_file = dir.path.join("pcb/width.circuit.json");
    let content = std::fs::read_to_string(&circuit_file).unwrap();
    let mut circuit: Value = serde_json::from_str(&content).unwrap();

    let mut modified = false;
    for elem in circuit.as_array_mut().unwrap() {
        if elem["type"] == "pcb_trace" {
            // Shrink trace width from 0.2 mm to 0.1 mm (below the 0.2 mm rule)
            elem["width"] = json!(0.1);
            if let Some(route) = elem.get_mut("route").and_then(|r| r.as_array_mut()) {
                for pt in route {
                    if pt.get("width").is_some() {
                        pt["width"] = json!(0.1);
                    }
                }
            }
            modified = true;
            break;
        }
    }
    assert!(modified, "must have found and modified a pcb_trace");
    std::fs::write(&circuit_file, serde_json::to_string_pretty(&circuit).unwrap()).unwrap();

    let out = run("pcb_drc", json!({"board": "width"}), &dir.path)
        .await
        .unwrap();
    let rpt = std::fs::read_to_string(dir.path.join("fab/width/width-drc.txt")).unwrap();
    let lines: Vec<&str> = rpt.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    assert!(lines.iter().any(|l| l.starts_with("track width: ")));
    assert_eq!(lines.last().copied(), Some("1 DRC error(s)."));
    assert!(out.text.contains("track width: "));
    assert!(out.text.contains("1 DRC error(s)."));
}

// -----------------------------------------------------------------------------
// Step 5 RF Calculators (Section 2)
// -----------------------------------------------------------------------------

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn calc_track_width_ipc2152() {
    // IPC-2221 (k=0.048, b=0.44, c=0.725, as KiCad's calculator):
    // Arithmetic:
    // A = (I / (k · ΔT^0.44))^(1 / 0.725)
    //   = (1.0 / (0.048 · 10^0.44))^(1 / 0.725) = 16.29 mil²
    // Thickness = 0.035 mm = 1.378 mil
    // Width = 16.29 mil² / 1.378 mil = 11.8 mil = 0.300 mm
    let out = run(
        "rf_calc",
        json!({
            "calc": "track_width",
            "current_a": 1.0,
            "temp_rise_c": 10.0,
            "copper_mm": 0.035,
            "layer": "external"
        }),
        Path::new("/nonexistent"),
    )
    .await
    .unwrap();
    let nums = parse_numbers_with_unit(&out.text, "mm");
    assert!(
        nums.iter().any(|&w| (w - 0.300).abs() <= 0.01),
        "expected track width 0.300 ± 0.01 mm in: {}\nparsed numbers: {:?}",
        out.text,
        nums
    );
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn calc_via_current_and_resistance() {
    // KiCad via resistance model:
    // drill = 0.3 mm, length = 1.6 mm, plating = 0.018 mm
    // resistance = 1.72e-5 Ω·mm × 1.6 / (π × 0.318 × 0.018) = 1.53 mΩ ± 0.05
    let out = run(
        "rf_calc",
        json!({
            "calc": "via",
            "drill_mm": 0.3,
            "length_mm": 1.6,
            "plating_mm": 0.018,
            "temp_rise_c": 10.0
        }),
        Path::new("/nonexistent"),
    )
    .await
    .unwrap();
    let m_nums = parse_numbers_with_unit(&out.text, "mΩ");
    let mohm_nums = parse_numbers_with_unit(&out.text, "mohm");
    let ohm_nums = parse_numbers_with_unit(&out.text, "Ω");
    let has_1_53 = m_nums.iter().any(|&r| (r - 1.53).abs() <= 0.05)
        || mohm_nums.iter().any(|&r| (r - 1.53).abs() <= 0.05)
        || ohm_nums.iter().any(|&r| (r * 1000.0 - 1.53).abs() <= 0.05);
    assert!(
        has_1_53,
        "expected via resistance 1.53 ± 0.05 mΩ in: {}\nparsed mΩ: {:?}, mohm: {:?}, Ω: {:?}",
        out.text,
        m_nums,
        mohm_nums,
        ohm_nums
    );
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn calc_fusing_onderdonk() {
    // Onderdonk formula (Tm 1083 °C, Ta 20 °C):
    // 0.5 × 0.035 mm = 34.54 circular mils
    // I = 34.54 × sqrt(log10(1063/254 + 1) / (33 × 1 s)) = 5.08 A ± 0.1
    let out = run(
        "rf_calc",
        json!({
            "calc": "fusing",
            "width_mm": 0.5,
            "copper_mm": 0.035,
            "time_s": 1.0
        }),
        Path::new("/nonexistent"),
    )
    .await
    .unwrap();
    let nums = parse_numbers_with_unit(&out.text, "A");
    assert!(
        nums.iter().any(|&i| (i - 5.08).abs() <= 0.1),
        "expected fusing current 5.08 ± 0.1 A in: {}\nparsed numbers: {:?}",
        out.text,
        nums
    );
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn calc_spacing_ipc2221b() {
    // Source: IPC-2221B Table 6-1 (Table B) 151-300 V external coated conductor (B4) requires 0.4 mm clearance
    let out = run(
        "rf_calc",
        json!({
            "calc": "spacing",
            "voltage_v": 250.0,
            "layer": "external_coated"
        }),
        Path::new("/nonexistent"),
    )
    .await
    .unwrap();
    let clearance_line = out
        .text
        .lines()
        .find(|l| {
            let lower = l.to_lowercase();
            lower.contains("clearance") || lower.contains("spacing")
        })
        .unwrap_or(&out.text);
    let nums = parse_numbers_with_unit(clearance_line, "mm");
    assert!(
        nums.iter().any(|&c| (c - 0.4).abs() <= 0.01),
        "expected clearance 0.4 mm in line '{clearance_line}' (output: {})\nparsed mm: {:?}",
        out.text,
        nums
    );
}

#[tokio::test]
#[ignore = "stage 3 not built yet"]
async fn calc_e_series_nearest_and_divider() {
    // 5000 Ω -> nearest E96 standard value is 4.99k (4990 Ω)
    let out = run(
        "rf_calc",
        json!({
            "calc": "e_series",
            "value": 5000.0,
            "series": "E96"
        }),
        Path::new("/nonexistent"),
    )
    .await
    .unwrap();
    let k_nums = parse_numbers_with_unit(&out.text, "k");
    let kohm_nums = parse_numbers_with_unit(&out.text, "kΩ");
    let ohm_nums = parse_numbers_with_unit(&out.text, "Ω");
    let has_4_99k = out.text.contains("4.99k")
        || k_nums.iter().any(|&v| (v - 4.99).abs() <= 0.01)
        || kohm_nums.iter().any(|&v| (v - 4.99).abs() <= 0.01)
        || ohm_nums.iter().any(|&v| (v - 4990.0).abs() <= 1.0);
    assert!(
        has_4_99k,
        "expected nearest E96 value 4.99k for 5000 Ω in: {}\nparsed k: {:?}, kΩ: {:?}, Ω: {:?}",
        out.text,
        k_nums,
        kohm_nums,
        ohm_nums
    );

    // Voltage divider: 5 V -> 3.3 V
    let out_div = run(
        "rf_calc",
        json!({
            "calc": "e_series",
            "vin_v": 5.0,
            "vout_v": 3.3,
            "series": "E96"
        }),
        Path::new("/nonexistent"),
    )
    .await
    .unwrap();
    let vout_line = out_div
        .text
        .lines()
        .find(|l| {
            let lower = l.to_lowercase();
            lower.contains("vout") || lower.contains("output") || lower.contains("v_out")
        });
    let vout_candidates = if let Some(line) = vout_line {
        parse_numbers_with_unit(line, "V")
    } else {
        parse_numbers_with_unit(&out_div.text, "V")
            .into_iter()
            .filter(|&v| (v - 5.0).abs() > 0.1)
            .collect()
    };
    assert!(
        !vout_candidates.is_empty(),
        "could not find reported output voltage in: {}",
        out_div.text
    );
    let vout = vout_candidates[0];
    let diff = (vout - 3.3).abs() / 3.3;
    assert!(
        diff <= 0.01,
        "reported output voltage {vout} V is not within 1% of 3.3 V (error: {:.2}%)",
        diff * 100.0
    );
}
