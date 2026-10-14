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
    assert!(content.contains("%MO"), "missing %MO in {}", path.display());
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
                    assert!(
                        !t.ends_with(".0"),
                        "Found redundant .0 in number '{t}': {line}"
                    );
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

mod fp {
    use forge_server::tools::pcb::footprint;

    #[test]
    fn fp_0402() {
        let fp = footprint("0402").expect("footprint 0402 should be generated");
        assert_eq!(fp.pads.len(), 2);
        let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
        let p2 = fp.pads.iter().find(|p| p.number == "2").unwrap();
        // 0402 nominal pad: 0.59 x 0.64 mm, opposite-pad centres: 0.97 mm
        assert!(
            (p1.size[0] - 0.59).abs() < 1e-4,
            "pad size x {} != 0.59",
            p1.size[0]
        );
        assert!(
            (p1.size[1] - 0.64).abs() < 1e-4,
            "pad size y {} != 0.64",
            p1.size[1]
        );
        let centres = (p2.at[0] - p1.at[0]).abs();
        assert!((centres - 0.97).abs() < 1e-4, "centres {} != 0.97", centres);
    }

    #[test]
    fn fp_0603() {
        let fp = footprint("0603").expect("footprint 0603 should be generated");
        assert_eq!(fp.pads.len(), 2);
        let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
        let p2 = fp.pads.iter().find(|p| p.number == "2").unwrap();
        // 0603 nominal pad: 0.875 x 0.95 mm, opposite-pad centres: 1.575 mm
        assert!(
            (p1.size[0] - 0.875).abs() < 1e-4,
            "pad size x {} != 0.875",
            p1.size[0]
        );
        assert!(
            (p1.size[1] - 0.95).abs() < 1e-4,
            "pad size y {} != 0.95",
            p1.size[1]
        );
        let centres = (p2.at[0] - p1.at[0]).abs();
        assert!(
            (centres - 1.575).abs() < 1e-4,
            "centres {} != 1.575",
            centres
        );
    }

    #[test]
    fn fp_soic8() {
        let fp = footprint("soic8").expect("footprint soic8 should be generated");
        assert_eq!(fp.pads.len(), 8);
        let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
        let p8 = fp.pads.iter().find(|p| p.number == "8").unwrap();
        // soic8 nominal pad: 1.95 x 0.60 mm, opposite-pad centres: 4.95 mm
        assert!(
            (p1.size[0] - 1.95).abs() < 1e-4,
            "pad size x {} != 1.95",
            p1.size[0]
        );
        assert!(
            (p1.size[1] - 0.60).abs() < 1e-4,
            "pad size y {} != 0.60",
            p1.size[1]
        );
        let centres = (p8.at[0] - p1.at[0]).abs();
        assert!((centres - 4.95).abs() < 1e-4, "centres {} != 4.95", centres);
    }

    #[test]
    fn fp_qfn32() {
        let fp = footprint("qfn32").expect("footprint qfn32 should be generated");
        assert_eq!(fp.pads.len(), 33);
        let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
        let p24 = fp.pads.iter().find(|p| p.number == "24").unwrap();
        // qfn32 nominal perimeter pad: 0.875 x 0.25 mm, opposite-pad centres: 4.875 mm
        assert!(
            (p1.size[0] - 0.875).abs() < 1e-4,
            "pad size x {} != 0.875",
            p1.size[0]
        );
        assert!(
            (p1.size[1] - 0.25).abs() < 1e-4,
            "pad size y {} != 0.25",
            p1.size[1]
        );
        let centres = (p24.at[0] - p1.at[0]).abs();
        assert!(
            (centres - 4.875).abs() < 1e-4,
            "centres {} != 4.875",
            centres
        );
        // Exposed pad (EP): 3.1 x 3.1 mm
        let ep = fp
            .pads
            .iter()
            .find(|p| p.number == "33" || p.number == "EP")
            .unwrap();
        assert!(
            (ep.size[0] - 3.1).abs() < 1e-4,
            "EP size x {} != 3.1",
            ep.size[0]
        );
        assert!(
            (ep.size[1] - 3.1).abs() < 1e-4,
            "EP size y {} != 3.1",
            ep.size[1]
        );
    }

    #[test]
    fn fp_usb_c_16() {
        let fp = footprint("usb_c_16").expect("footprint usb_c_16 should be generated");
        assert_eq!(fp.pads.len(), 22);
        let p_a1 = fp.pads.iter().find(|p| p.number == "A1").unwrap();
        assert!((p_a1.at[0] - (-3.25)).abs() < 1e-4);
        assert!((p_a1.at[1] - (-4.045)).abs() < 1e-4);
        assert!((p_a1.size[0] - 0.6).abs() < 1e-4);
        assert!((p_a1.size[1] - 1.45).abs() < 1e-4);
        assert!(p_a1.drill.is_none());
        assert!(!p_a1.round);
        assert!(p_a1.plated);

        let p_a5 = fp.pads.iter().find(|p| p.number == "A5").unwrap();
        assert!((p_a5.at[0] - (-1.25)).abs() < 1e-4);
        assert!((p_a5.size[0] - 0.3).abs() < 1e-4);

        let p_b12 = fp.pads.iter().find(|p| p.number == "B12").unwrap();
        assert!((p_b12.at[0] - (-3.25)).abs() < 1e-4);
        assert!((p_b12.at[1] - (-4.045)).abs() < 1e-4);

        let s1_pads: Vec<_> = fp.pads.iter().filter(|p| p.number == "S1").collect();
        assert_eq!(s1_pads.len(), 4);
        let s1 = s1_pads
            .iter()
            .find(|p| (p.at[0] - (-4.32)).abs() < 1e-4 && (p.at[1] - (-3.13)).abs() < 1e-4)
            .unwrap();
        assert!((s1.size[0] - 1.0).abs() < 1e-4);
        assert!((s1.size[1] - 2.1).abs() < 1e-4);
        assert_eq!(s1.drill, Some([0.6, 1.7]));
        assert!(s1.round);
        assert!(s1.plated);

        let npth = fp.pads.iter().find(|p| !p.plated).unwrap();
        assert!((npth.at[0].abs() - 2.89).abs() < 1e-4);
        assert!((npth.at[1] - (-2.6)).abs() < 1e-4);
        assert_eq!(npth.drill, Some([0.65, 0.65]));
        assert!(!npth.plated);

        for p in &fp.pads {
            assert!(
                p.at[0] - p.size[0] / 2.0 >= fp.courtyard[0] - 1e-4,
                "pad {} xmin outside courtyard",
                p.number
            );
            assert!(
                p.at[1] - p.size[1] / 2.0 >= fp.courtyard[1] - 1e-4,
                "pad {} ymin outside courtyard",
                p.number
            );
            assert!(
                p.at[0] + p.size[0] / 2.0 <= fp.courtyard[2] + 1e-4,
                "pad {} xmax outside courtyard",
                p.number
            );
            assert!(
                p.at[1] + p.size[1] / 2.0 <= fp.courtyard[3] + 1e-4,
                "pad {} ymax outside courtyard",
                p.number
            );
        }
    }

    #[test]
    fn fp_usb_micro_b() {
        let fp = footprint("usb_micro_b").expect("footprint usb_micro_b should be generated");
        assert_eq!(fp.pads.len(), 13);
        let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
        assert!((p1.at[0] - (-1.3)).abs() < 1e-4);
        assert!((p1.at[1] - (-1.4625)).abs() < 1e-4);
        assert!((p1.size[0] - 0.4).abs() < 1e-4);
        assert!((p1.size[1] - 1.35).abs() < 1e-4);
        assert!(p1.drill.is_none());

        let p5 = fp.pads.iter().find(|p| p.number == "5").unwrap();
        assert!((p5.at[0] - 1.3).abs() < 1e-4);

        let p6_pads: Vec<_> = fp.pads.iter().filter(|p| p.number == "6").collect();
        assert_eq!(p6_pads.len(), 8);
        let slot = p6_pads
            .iter()
            .find(|p| (p.at[0] - (-3.5)).abs() < 1e-4)
            .unwrap();
        assert_eq!(slot.drill, Some([0.6, 1.3]));
        assert!((slot.size[0] - 1.2).abs() < 1e-4);
        assert!((slot.size[1] - 1.9).abs() < 1e-4);

        for p in &fp.pads {
            assert!(
                p.at[0] - p.size[0] / 2.0 >= fp.courtyard[0] - 1e-4,
                "pad {} xmin outside courtyard",
                p.number
            );
            assert!(
                p.at[1] - p.size[1] / 2.0 >= fp.courtyard[1] - 1e-4,
                "pad {} ymin outside courtyard",
                p.number
            );
            assert!(
                p.at[0] + p.size[0] / 2.0 <= fp.courtyard[2] + 1e-4,
                "pad {} xmax outside courtyard",
                p.number
            );
            assert!(
                p.at[1] + p.size[1] / 2.0 <= fp.courtyard[3] + 1e-4,
                "pad {} ymax outside courtyard",
                p.number
            );
        }
    }

    #[test]
    fn fp_jst_ph() {
        for n in 2..=16 {
            let name = format!("jst_ph_{n}");
            let fp = footprint(&name).expect("footprint should exist");
            assert_eq!(fp.pads.len(), n);
            let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
            assert!(!p1.round, "pin 1 of {name} should be rectangular-marked");
            assert_eq!(p1.drill, Some([0.75, 0.75]));
            assert!((p1.size[0] - 1.2).abs() < 1e-4);
            assert!((p1.size[1] - 1.75).abs() < 1e-4);

            let p2 = fp.pads.iter().find(|p| p.number == "2").unwrap();
            assert!(p2.round, "pin 2 of {name} should be round");
            assert_eq!(p2.drill, Some([0.75, 0.75]));
            assert!((p2.size[0] - 1.2).abs() < 1e-4);
            assert!((p2.size[1] - 1.75).abs() < 1e-4);
            assert!((p2.at[0] - p1.at[0] - 2.0).abs() < 1e-4);

            for p in &fp.pads {
                assert!(
                    p.at[0] - p.size[0] / 2.0 >= fp.courtyard[0] - 1e-4,
                    "pad {} xmin outside courtyard for {name}",
                    p.number
                );
                assert!(
                    p.at[1] - p.size[1] / 2.0 >= fp.courtyard[1] - 1e-4,
                    "pad {} ymin outside courtyard for {name}",
                    p.number
                );
                assert!(
                    p.at[0] + p.size[0] / 2.0 <= fp.courtyard[2] + 1e-4,
                    "pad {} xmax outside courtyard for {name}",
                    p.number
                );
                assert!(
                    p.at[1] + p.size[1] / 2.0 <= fp.courtyard[3] + 1e-4,
                    "pad {} ymax outside courtyard for {name}",
                    p.number
                );
            }
        }
    }

    #[test]
    fn fp_barrel_jack() {
        let fp = footprint("barrel_jack").expect("footprint barrel_jack should be generated");
        assert_eq!(fp.pads.len(), 3);
        let p1 = fp.pads.iter().find(|p| p.number == "1").unwrap();
        assert!(!p1.round, "pin 1 should be rectangular-marked");
        assert_eq!(p1.drill, Some([1.6, 1.6]));
        assert!((p1.at[0] - 0.0).abs() < 1e-4);
        assert!((p1.at[1] - 0.0).abs() < 1e-4);
        assert!((p1.size[0] - 2.6).abs() < 1e-4);
        assert!((p1.size[1] - 2.6).abs() < 1e-4);

        let p2 = fp.pads.iter().find(|p| p.number == "2").unwrap();
        assert!(p2.round);
        assert_eq!(p2.drill, Some([1.6, 1.6]));
        assert!((p2.at[0] - 0.0).abs() < 1e-4);
        assert!((p2.at[1] - 6.0).abs() < 1e-4);

        let p3 = fp.pads.iter().find(|p| p.number == "3").unwrap();
        assert!(p3.round);
        assert_eq!(p3.drill, Some([1.6, 1.6]));
        assert!((p3.at[0] - 4.7).abs() < 1e-4);
        assert!((p3.at[1] - 3.0).abs() < 1e-4);

        for p in &fp.pads {
            assert!(
                p.at[0] - p.size[0] / 2.0 >= fp.courtyard[0] - 1e-4,
                "pad {} xmin outside courtyard",
                p.number
            );
            assert!(
                p.at[1] - p.size[1] / 2.0 >= fp.courtyard[1] - 1e-4,
                "pad {} ymin outside courtyard",
                p.number
            );
            assert!(
                p.at[0] + p.size[0] / 2.0 <= fp.courtyard[2] + 1e-4,
                "pad {} xmax outside courtyard",
                p.number
            );
            assert!(
                p.at[1] + p.size[1] / 2.0 <= fp.courtyard[3] + 1e-4,
                "pad {} ymax outside courtyard",
                p.number
            );
        }
    }
}

// -----------------------------------------------------------------------------
// Step 2 Spec Validation Errors (Section 3.5)
// -----------------------------------------------------------------------------

#[tokio::test]
async fn spec_error_source_must_be_pcb_json() {
    let dir = project("spec_err1", &[("pcb/invalid.txt", "{}")]);
    let err = run("pcb_build", json!({"source": "pcb/invalid.txt"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("source must be a .pcb.json file"));
}

#[tokio::test]
async fn spec_error_source_does_not_exist() {
    let dir = project("spec_err2", &[] as &[(&str, &str)]);
    let err = run(
        "pcb_build",
        json!({"source": "pcb/missing.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err
        .to_string()
        .contains("pcb/missing.pcb.json does not exist; write it with write_file first"));
}

#[tokio::test]
async fn spec_error_invalid_name() {
    let dir = project("spec_err3", &[("pcb/bad space.pcb.json", "{}")]);
    let err = run(
        "pcb_build",
        json!({"source": "pcb/bad space.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err
        .to_string()
        .contains("name may only use letters, digits, - and _"));
}

#[tokio::test]
async fn spec_error_unknown_pin() {
    let bad_board = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [0, 0]}
  ],
  "nets": {"IN": ["R1.3"]}
}"#;
    let dir = project("spec_err4", &[("pcb/bad_pin.pcb.json", bad_board)]);
    let err = run(
        "pcb_build",
        json!({"source": "pcb/bad_pin.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("R1.3: R1 (0402) has pins 1, 2"));
}

#[tokio::test]
async fn spec_error_unknown_footprint() {
    let bad_board = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "nonexistent_fp_xyz", "at": [0, 0]}
  ],
  "nets": {}
}"#;
    let dir = project("spec_err5", &[("pcb/bad_fp.pcb.json", bad_board)]);
    let err = run(
        "pcb_build",
        json!({"source": "pcb/bad_fp.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err
        .to_string()
        .contains("unknown footprint nonexistent_fp_xyz; use one of:"));
}

#[tokio::test]
async fn spec_error_part_off_board() {
    let bad_board = r#"{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [100, 100]}
  ],
  "nets": {}
}"#;
    let dir = project("spec_err6", &[("pcb/off_board.pcb.json", bad_board)]);
    let err = run(
        "pcb_build",
        json!({"source": "pcb/off_board.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("R1 is off the board"));
}

#[tokio::test]
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
    let err = run(
        "pcb_build",
        json!({"source": "pcb/overlap.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("parts overlap: R1 and R2"));
}

// -----------------------------------------------------------------------------
// Step 3 Router & Pours (Section 3.4 & unroutable error in 3.5)
// -----------------------------------------------------------------------------

#[tokio::test]
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
    let dir = project(
        "route_unroutable",
        &[("pcb/unroutable.pcb.json", unroutable_board)],
    );
    let err = run(
        "pcb_build",
        json!({"source": "pcb/unroutable.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err
        .to_string()
        .contains("unroutable: net SIG (R1.1 to R2.1)"));
}

#[tokio::test]
async fn route_led_board() {
    let dir = project("route_led", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.text.contains("No errors: every connection is routed."));
    assert!(out
        .text
        .contains("board 30 × 20 mm, 1.6 mm thick, 2 copper layers."));
}

#[tokio::test]
async fn route_555_blinker_no_errors() {
    let dir = project(
        "route_555",
        &[("pcb/blinker.pcb.json", BLINKER_555_BOARD_JSON)],
    );
    let out = run(
        "pcb_build",
        json!({"source": "pcb/blinker.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.text.contains("No errors: every connection is routed."));
    assert!(!out.text.contains("ERROR"));
}

#[tokio::test]
async fn route_4layer_board() {
    let dir = project("route_4l", &[("pcb/four.pcb.json", FOUR_LAYER_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/four.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.text.contains("4 copper layers."));
    assert!(out.text.contains("No errors: every connection is routed."));
}

#[tokio::test]
async fn route_auto_placed_board_no_at() {
    let dir = project(
        "route_autoplace",
        &[("pcb/auto.pcb.json", AUTO_PLACED_BOARD_JSON)],
    );
    let out = run(
        "pcb_build",
        json!({"source": "pcb/auto.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.text.contains("No errors: every connection is routed."));
    let board_json: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path.join("pcb/auto.board.json")).unwrap(),
    )
    .unwrap();
    let components = board_json["components"]
        .as_array()
        .expect("components array");
    assert_eq!(components.len(), 3);
    for comp in components {
        let min_x = comp["min"][0].as_f64().unwrap();
        let max_x = comp["max"][0].as_f64().unwrap();
        let min_y = comp["min"][1].as_f64().unwrap();
        let max_y = comp["max"][1].as_f64().unwrap();
        assert!(
            min_x >= -15.0 && max_x <= 15.0,
            "component outside x bounds: {min_x}..{max_x}"
        );
        assert!(
            min_y >= -10.0 && max_y <= 10.0,
            "component outside y bounds: {min_y}..{max_y}"
        );
    }
}

#[tokio::test]
async fn pour_ground_plane_generates_g36() {
    let dir = project("pour_gnd", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.text.contains("Copper pours: GND on bottom"));
    let b_cu = std::fs::read_to_string(dir.path.join("fab/led/led-B_Cu.gbr")).unwrap();
    assert!(
        b_cu.contains("G36"),
        "bottom copper Gerber must contain G36 region code for pour"
    );
}

#[tokio::test]
async fn pour_antenna_clears_ground_under_keepout() {
    let dir = project("pour_ant", &[("pcb/ant.pcb.json", ANTENNA_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/ant.pcb.json"}),
        &dir.path,
    )
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
    assert!(
        max_y <= 12.1,
        "ground reaches y {max_y} under the antenna (must be <= 12.1)"
    );
}

// -----------------------------------------------------------------------------
// Step 4 Outputs & Reports (Section 3.3)
// -----------------------------------------------------------------------------

#[tokio::test]
async fn out_555_build_time_and_connection_count() {
    let dir = project(
        "out_555",
        &[("pcb/blinker.pcb.json", BLINKER_555_BOARD_JSON)],
    );
    let start = std::time::Instant::now();
    let out = run(
        "pcb_build",
        json!({"source":"pcb/blinker.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    eprintln!(
        "555 board native build: {:.3} ms (routing, all exports and file writes)",
        start.elapsed().as_secs_f64() * 1000.
    );
    assert!(
        out.text.contains("7 net(s) routed as 15 trace(s)"),
        "{}",
        out.text
    );
    assert_eq!(out.files.len(), 21);
}

#[tokio::test]
async fn out_circuit_matches_the_rf_reader_for_every_antenna_shape() {
    use forge_server::tools::exec::{find_program, run as exec};
    if tokio::task::spawn_blocking(|| find_program(&["python3".into()]))
        .await
        .unwrap()
        .is_none()
    {
        eprintln!("skipped: python3 is not installed");
        return;
    }
    let reader = include_str!("../runners/openems_antenna.py")
        .split("# ---------- mesh ----------")
        .next()
        .unwrap()
        .replace("import numpy as np", "");
    let check = format!("{reader}\nboard, z, name, copper, barrels, pours, pads, arm = load_board(json.load(open(sys.argv[1])), 'ANT1')\nassert name == 'ANT1' and arm and copper and pours and 'pin1' in pads\nassert set(z) == {{'top', 'bottom', 'inner1', 'inner2'}}\nassert feed_port(z, pads, pours)[2] in ('x', 'y', 'z')\n");
    for shape in [
        "2.4ghz_quarter_wave_monopole",
        "2.4ghz_meandered_monopole",
        "2.4ghz_inverted_f",
        "2.4ghz_meandered_inverted_f",
        "2.4ghz_folded_dipole",
    ] {
        let json = json!({"board":{"width":80,"height":60,"layers":4},"parts":[{"ref":"ANT1","kind":"antenna","shape":shape,"at":[0,0]}],"pours":[{"net":"GND","layer":"bottom"}]}).to_string();
        let dir = project("out_rf_reader", &[("pcb/ant.pcb.json", json)]);
        run("pcb_build", json!({"source":"pcb/ant.pcb.json"}), &dir.path)
            .await
            .unwrap();
        let result = exec(
            "python3",
            &["-c".into(), check.clone(), "pcb/ant.circuit.json".into()],
            &dir.path,
            30_000,
            &[],
        )
        .await;
        assert_eq!(result.code, 0, "{shape}: {}", result.stderr);
    }
}

#[tokio::test]
async fn out_file_pcb_svg() {
    let dir = project("out_pcb_svg", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.files.contains(&"pcb/led-pcb.svg".into()));
    let content = std::fs::read_to_string(dir.path.join("pcb/led-pcb.svg")).unwrap();
    assert!(content.contains("<svg") && content.contains("</svg>"));
}

#[tokio::test]
async fn out_file_schematic_svg() {
    let dir = project("out_sch_svg", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.files.contains(&"pcb/led-schematic.svg".into()));
    let content = std::fs::read_to_string(dir.path.join("pcb/led-schematic.svg")).unwrap();
    assert!(content.contains("<svg") && content.contains("</svg>"));
}

#[tokio::test]
async fn out_file_glb() {
    let dir = project("out_glb", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.files.contains(&"pcb/led.glb".into()));
    let bytes = std::fs::read(dir.path.join("pcb/led.glb")).unwrap();
    assert!(
        bytes.starts_with(b"glTF"),
        "GLB must start with glTF binary magic"
    );
}

#[tokio::test]
async fn out_file_step() {
    let dir = project("out_step", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.files.contains(&"pcb/led.step".into()));
    let content = std::fs::read_to_string(dir.path.join("pcb/led.step")).unwrap();
    assert!(
        content.contains("ISO-10303-21"),
        "STEP must contain ISO-10303-21 header"
    );
}

#[tokio::test]
async fn out_file_board_json() {
    let dir = project("out_board_json", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
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
async fn out_file_circuit_json() {
    let dir = project("out_circuit_json", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
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
async fn out_file_fab_copper_f_cu_gerber() {
    let dir = project("out_f_cu", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-F_Cu.gbr"));
}

#[tokio::test]
async fn out_file_fab_copper_b_cu_gerber() {
    let dir = project("out_b_cu", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-B_Cu.gbr"));
}

#[tokio::test]
async fn out_file_fab_copper_in1_cu_4layer_gerber() {
    let dir = project("out_in1", &[("pcb/four.pcb.json", FOUR_LAYER_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/four.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/four/four-In1_Cu.gbr"));
}

#[tokio::test]
async fn out_file_fab_copper_in2_cu_4layer_gerber() {
    let dir = project("out_in2", &[("pcb/four.pcb.json", FOUR_LAYER_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/four.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/four/four-In2_Cu.gbr"));
}

#[tokio::test]
async fn out_file_fab_mask_f_mask_gerber() {
    let dir = project("out_f_mask", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-F_Mask.gbr"));
}

#[tokio::test]
async fn out_file_fab_mask_b_mask_gerber() {
    let dir = project("out_b_mask", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-B_Mask.gbr"));
}

#[tokio::test]
async fn out_file_fab_paste_f_paste_gerber() {
    let dir = project("out_f_paste", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-F_Paste.gbr"));
}

#[tokio::test]
async fn out_file_fab_paste_b_paste_gerber() {
    let dir = project("out_b_paste", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-B_Paste.gbr"));
}

#[tokio::test]
async fn out_file_fab_silkscreen_f_silkscreen_gerber() {
    let dir = project("out_f_silk", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-F_Silkscreen.gbr"));
}

#[tokio::test]
async fn out_file_fab_silkscreen_b_silkscreen_gerber() {
    let dir = project("out_b_silk", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-B_Silkscreen.gbr"));
}

#[tokio::test]
async fn out_file_fab_edge_cuts_gerber() {
    let dir = project("out_edge", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_gerber_syntax(&dir.path.join("fab/led/led-Edge_Cuts.gbr"));
}

#[tokio::test]
async fn out_file_fab_drill_pth_excellon() {
    let dir = project("out_pth", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_excellon_syntax(&dir.path.join("fab/led/led-PTH.drl"));
}

#[tokio::test]
async fn out_file_fab_drill_npth_excellon() {
    let dir = project("out_npth", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_excellon_syntax(&dir.path.join("fab/led/led-NPTH.drl"));
}

#[tokio::test]
async fn out_file_fab_pnp_csv() {
    let dir = project("out_pnp", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let content = std::fs::read_to_string(dir.path.join("fab/led/led-pnp.csv")).unwrap();
    assert!(content.starts_with("Designator,Mid X,Mid Y,Layer,Rotation"));
}

#[tokio::test]
async fn out_file_fab_bom_csv() {
    let dir = project("out_bom", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let content = std::fs::read_to_string(dir.path.join("fab/led/led-bom.csv")).unwrap();
    assert!(content.starts_with("Comment,Designator,Footprint,LCSC Part #"));
}

#[tokio::test]
async fn out_report_line_built_board() {
    let dir = project("out_rep_board", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out
        .text
        .contains("Built pcb/led.pcb.json: board 30 × 20 mm, 1.6 mm thick, 2 copper layers."));
}

#[tokio::test]
async fn out_report_line_parts() {
    let dir = project("out_rep_parts", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.text.contains("4 part(s):"));
}

#[tokio::test]
async fn out_report_line_routed_traces() {
    let dir = project("out_rep_traces", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(
        out.text.contains("routed as")
            && out.text.contains("trace(s) with")
            && out.text.contains("via(s);")
            && out.text.contains("hole(s).")
    );
}

#[tokio::test]
async fn out_report_line_copper_pours() {
    let dir = project("out_rep_pours", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.text.contains("Copper pours: GND on bottom"));
}

#[tokio::test]
async fn out_report_line_no_errors() {
    let dir = project("out_rep_no_err", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    assert!(out.text.contains("No errors: every connection is routed."));
}

#[tokio::test]
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
    let err = run(
        "pcb_build",
        json!({"source": "pcb/err.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err
        .to_string()
        .contains("1 ERROR(S), fix them and rebuild:"));
}

// -----------------------------------------------------------------------------
// Step 5 DRC (Section 2 & 3.5)
// -----------------------------------------------------------------------------

#[tokio::test]
async fn drc_error_board_not_built() {
    let dir = project("drc_unbuilt", &[] as &[(&str, &str)]);
    let err = run("pcb_drc", json!({"board": "unbuilt"}), &dir.path)
        .await
        .unwrap_err();
    assert!(err
        .to_string()
        .contains("board unbuilt is not built; run pcb_build first"));
}

#[tokio::test]
async fn drc_clean_board() {
    let dir = project("drc_clean", &[("pcb/led.pcb.json", LED_BOARD_JSON)]);
    run(
        "pcb_build",
        json!({"source": "pcb/led.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let out = run("pcb_drc", json!({"board": "led"}), &dir.path)
        .await
        .unwrap();
    let drc_path = dir.path.join("fab/led/led-drc.txt");
    assert!(drc_path.exists(), "fab/led/led-drc.txt must exist");
    let rpt = std::fs::read_to_string(&drc_path).unwrap();
    let lines: Vec<&str> = rpt
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert_eq!(lines.last().copied(), Some("No DRC errors."));
    assert!(out.text.contains("No DRC errors."));
}

#[tokio::test]
async fn drc_broken_board_reports_violations() {
    let dir = project(
        "drc_broken",
        &[("pcb/broken.pcb.json", TWO_RESISTOR_BOARD_JSON)],
    );
    run(
        "pcb_build",
        json!({"source": "pcb/broken.pcb.json"}),
        &dir.path,
    )
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
    std::fs::write(
        &circuit_file,
        serde_json::to_string_pretty(&circuit).unwrap(),
    )
    .unwrap();

    let out = run("pcb_drc", json!({"board": "broken"}), &dir.path)
        .await
        .unwrap();
    let rpt = std::fs::read_to_string(dir.path.join("fab/broken/broken-drc.txt")).unwrap();
    let lines: Vec<&str> = rpt
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert!(lines.iter().any(|l| l.starts_with("clearance: ")));
    // The moved pad may also crowd the routed tracks, so only require at least one error.
    let last = lines.last().copied().unwrap_or("");
    assert!(
        last.ends_with(" DRC error(s).") && !last.starts_with("0 "),
        "{rpt}"
    );
    assert!(out.text.contains("clearance: "));
}

#[tokio::test]
async fn drc_track_width_violation() {
    let dir = project(
        "drc_width",
        &[("pcb/width.pcb.json", TWO_RESISTOR_BOARD_JSON)],
    );
    run(
        "pcb_build",
        json!({"source": "pcb/width.pcb.json"}),
        &dir.path,
    )
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
    std::fs::write(
        &circuit_file,
        serde_json::to_string_pretty(&circuit).unwrap(),
    )
    .unwrap();

    let out = run("pcb_drc", json!({"board": "width"}), &dir.path)
        .await
        .unwrap();
    let rpt = std::fs::read_to_string(dir.path.join("fab/width/width-drc.txt")).unwrap();
    let lines: Vec<&str> = rpt
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    assert!(lines.iter().any(|l| l.starts_with("track width: ")));
    assert_eq!(lines.last().copied(), Some("1 DRC error(s)."));
    assert!(out.text.contains("track width: "));
    assert!(out.text.contains("1 DRC error(s)."));
}

// -----------------------------------------------------------------------------
// Step 5 RF Calculators (Section 2)
// -----------------------------------------------------------------------------

#[tokio::test]
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
        out.text, m_nums, mohm_nums, ohm_nums
    );
}

#[tokio::test]
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
        out.text, k_nums, kohm_nums, ohm_nums
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
    let vout_line = out_div.text.lines().find(|l| {
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

#[tokio::test]
async fn route_555_vias_fan_out_clear_of_smd_pads_and_pass_drc() {
    let dir = project(
        "route_555_fanout",
        &[("pcb/blinker.pcb.json", BLINKER_555_BOARD_JSON)],
    );
    run(
        "pcb_build",
        json!({"source":"pcb/blinker.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();
    let circuit: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path.join("pcb/blinker.circuit.json")).unwrap(),
    )
    .unwrap();
    let elements = circuit.as_array().unwrap();
    let vias: Vec<_> = elements.iter().filter(|e| e["type"] == "pcb_via").collect();
    assert!(!vias.is_empty(), "555 board must exercise layer changes");
    for via in vias {
        let x = via["x"].as_f64().unwrap();
        let y = via["y"].as_f64().unwrap();
        eprintln!("555 via {}: [{x}, {y}] mm", via["net"]);
        let margin = via["outer_diameter"].as_f64().unwrap() / 2. + 0.2;
        for pad in elements.iter().filter(|e| e["type"] == "pcb_smtpad") {
            let dx = x - pad["x"].as_f64().unwrap();
            let dy = y - pad["y"].as_f64().unwrap();
            let (s, c) = pad["rotation"].as_f64().unwrap().to_radians().sin_cos();
            let local_x = c * dx + s * dy;
            let local_y = -s * dx + c * dy;
            let half_width = pad["width"].as_f64().unwrap() / 2.;
            let half_height = pad["height"].as_f64().unwrap() / 2.;
            assert!(
                local_x.abs() > half_width + 0.2 || local_y.abs() > half_height + 0.2,
                "via {} lies inside expanded SMD pad {}",
                via["pcb_via_id"],
                pad["pcb_smtpad_id"]
            );
            let gap = (local_x.abs() - half_width)
                .max(0.)
                .hypot((local_y.abs() - half_height).max(0.));
            assert!(
                gap >= margin - 1e-6,
                "via {} copper crowds SMD pad {}",
                via["pcb_via_id"],
                pad["pcb_smtpad_id"]
            );
        }
    }
    let drc = run("pcb_drc", json!({"board":"blinker"}), &dir.path)
        .await
        .unwrap();
    assert_eq!(drc.text, "No DRC errors.");
}

#[tokio::test]
async fn calc_rejects_invalid_pcb_inputs() {
    for input in [
        json!({"calc":"track_width","current_a":-1,"temp_rise_c":10,"copper_mm":0.035}),
        json!({"calc":"track_width","current_a":1,"temp_rise_c":10,"copper_mm":0.035,"layer":"external_coated"}),
        json!({"calc":"via","drill_mm":0.3,"length_mm":1.6,"plating_mm":0,"temp_rise_c":10}),
        json!({"calc":"fusing","width_mm":0.5,"copper_mm":0.035,"time_s":0}),
        json!({"calc":"spacing","voltage_v":-1}),
        json!({"calc":"e_series","value":0,"series":"E96"}),
        json!({"calc":"e_series","vin_v":5,"vout_v":5,"series":"E96"}),
    ] {
        assert!(
            run("rf_calc", input.clone(), Path::new("/nonexistent"))
                .await
                .is_err(),
            "{input}"
        );
    }
}

#[tokio::test]
async fn route_connector_board_with_usb_c_barrel_jack_jst_ph() {
    let board_json = json!({
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
    })
    .to_string();

    let dir = project("connector_board", &[("pcb/conn.pcb.json", &board_json)]);
    let out = run(
        "pcb_build",
        json!({"source": "pcb/conn.pcb.json"}),
        &dir.path,
    )
    .await
    .unwrap();

    assert!(
        out.text.contains("No errors: every connection is routed."),
        "Routing output: {}",
        out.text
    );

    // Excellon contains G85 (slotted routed holes)
    let pth_drill = std::fs::read_to_string(dir.path.join("fab/conn/conn-PTH.drl")).unwrap();
    assert!(
        pth_drill.contains("G85"),
        "PTH Excellon drill file must contain G85 routed slot commands"
    );

    // Gerber copper contains obround aperture definition: %ADD...O,...X...*%
    let f_cu = std::fs::read_to_string(dir.path.join("fab/conn/conn-F_Cu.gbr")).unwrap();
    assert!(
        f_cu.lines()
            .any(|l| l.starts_with("%ADD") && l.contains('O') && l.ends_with("*%")),
        "F_Cu Gerber must define an obround aperture with %ADD...O,...X...*%"
    );

    // pcb_drc reports clean
    let drc_out = run("pcb_drc", json!({"board": "conn"}), &dir.path)
        .await
        .unwrap();
    assert!(
        drc_out.text.contains("No DRC errors."),
        "DRC report: {}",
        drc_out.text
    );
}

#[tokio::test]
async fn oval_pads_flash_on_the_right_angle_grid_and_stay_regions_otherwise() {
    for (rotation, expected) in [(90, Some("O,1.75X1.2*%")), (45, None)] {
        let board_json = json!({
            "board": { "width": 20, "height": 20, "layers": 2, "thickness": 1.6 },
            "parts": [{ "ref": "J1", "kind": "header", "footprint": "jst_ph_2", "at": [0, 0], "rotation": rotation }],
            "nets": {}
        })
        .to_string();
        let dir = project("oval_rotation", &[("pcb/oval.pcb.json", &board_json)]);
        run("pcb_build", json!({"source": "pcb/oval.pcb.json"}), &dir.path)
            .await
            .unwrap();
        let f_cu = std::fs::read_to_string(dir.path.join("fab/oval/oval-F_Cu.gbr")).unwrap();
        let oval = f_cu.lines().find(|l| l.starts_with("%ADD") && l.contains("O,"));
        assert_eq!(oval.map(|l| &l[l.find("O,").unwrap()..]), expected, "{rotation}");
    }
}

// The 6-part board from route.rs negotiation test: shortest-first routing walls off three nets.
#[tokio::test]
async fn route_congested_board_negotiates_every_net_drc_clean() {
    let parts: Vec<_> = (0..6)
        .map(|i| {
            let (kind, footprint) = [("chip", "soic8"), ("chip", "qfn32"), ("resistor", "0603")][i % 3];
            json!({"ref": format!("P{}", i + 1), "kind": kind, "footprint": footprint,
                   "at": [-20 + i as i64 * 8, 0], "side": if i % 5 == 0 { "bottom" } else { "top" }})
        })
        .collect();
    let board_json = json!({
        "board": { "width": 50, "height": 10, "layers": 2, "thickness": 1.6 },
        "rules": { "trace": 0.2, "clearance": 0.15, "via": [0.6, 0.3], "edge": 0.3 },
        "parts": parts,
        "nets": {
            "N00": ["P5.7", "P5.3"], "N01": ["P2.5", "P5.13"], "N02": ["P2.24", "P2.28"],
            "N03": ["P4.5", "P2.25"], "N04": ["P5.5", "P1.3"], "N05": ["P5.30", "P2.2"],
            "N06": ["P4.4", "P4.6"], "N07": ["P2.30", "P5.11"], "N08": ["P1.5", "P2.9"],
            "N09": ["P2.6", "P5.18"], "N10": ["P5.23", "P5.31"], "N11": ["P4.1", "P2.12"]
        }
    })
    .to_string();
    let dir = project("congested", &[("pcb/congested.pcb.json", &board_json)]);
    let out = run("pcb_build", json!({"source": "pcb/congested.pcb.json"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("No errors: every connection is routed."), "{}", out.text);
    let drc = run("pcb_drc", json!({"board": "congested"}), &dir.path).await.unwrap();
    assert!(drc.text.contains("No DRC errors."), "{}", drc.text);
}

#[tokio::test]
async fn route_exposed_pad_with_opt_in_filled_vias_and_drc() {
    let mut input = json!({"board":{"width":16,"height":12},"rules":{"clearance":0.15},"parts":[
        {"ref":"U1","kind":"chip","footprint":"qfn32","at":[0,0]},
        {"ref":"R1","kind":"resistor","footprint":"0603","at":[5,0]}],
        "nets":{"GND":["U1.33","R1.1"]}});
    let dir = project("filled_vias", &[("pcb/ep.pcb.json", &input.to_string())]);
    let failed = run("pcb_build", json!({"source":"pcb/ep.pcb.json"}), &dir.path).await.unwrap_err().to_string();
    assert!(failed.contains("unroutable: net GND"), "{failed}");
    input["rules"]["via_in_pad"] = json!(true);
    std::fs::write(dir.path.join("pcb/ep.pcb.json"), input.to_string()).unwrap();
    run("pcb_build", json!({"source":"pcb/ep.pcb.json"}), &dir.path).await.unwrap();
    let drc = run("pcb_drc", json!({"board":"ep"}), &dir.path).await.unwrap();
    assert_eq!(drc.text, "erc: R1.2 is not connected\nFilled/capped vias-in-pad are required.\n1 ERC warning(s).\nNo DRC errors.");
    let original = std::fs::read_to_string(dir.path.join("pcb/ep.circuit.json")).unwrap();
    let mut circuit: Value = serde_json::from_str(&original).unwrap();
    let via = circuit.as_array_mut().unwrap().iter_mut().find(|e| {
        e["type"] == "pcb_via"
            && (e["x"].as_f64().unwrap().abs() - 1.55).max(0.)
                .hypot((e["y"].as_f64().unwrap().abs() - 1.55).max(0.)) <= 0.3 + 1e-6
    }).unwrap();
    via["net"] = json!("OTHER");
    std::fs::write(dir.path.join("pcb/ep.circuit.json"), circuit.to_string()).unwrap();
    let drc = run("pcb_drc", json!({"board":"ep"}), &dir.path).await.unwrap();
    assert!(drc.text.contains("via in pad: via OTHER in U1.33"), "{}", drc.text);
    circuit.as_array_mut().unwrap().iter_mut().filter(|e| e["type"] == "pcb_via")
        .for_each(|e| e["net"] = Value::Null);
    std::fs::write(dir.path.join("pcb/ep.circuit.json"), circuit.to_string()).unwrap();
    let drc = run("pcb_drc", json!({"board":"ep"}), &dir.path).await.unwrap();
    assert!(drc.text.contains("via in pad: via unconnected in U1.33"), "{}", drc.text);
    std::fs::write(dir.path.join("pcb/ep.circuit.json"), original).unwrap();
    input["rules"]["via_in_pad"] = json!(false);
    std::fs::write(dir.path.join("pcb/ep.pcb.json"), input.to_string()).unwrap();
    let drc = run("pcb_drc", json!({"board":"ep"}), &dir.path).await.unwrap();
    assert!(drc.text.contains("via in pad: via GND in U1.33"), "{}", drc.text);
    assert!(!drc.text.contains("Filled/capped"));
}


fn nearest_benchmark_pin(pins: &mut Vec<(String, [f64; 2])>, target: [f64; 2]) -> String {
    let index = (0..pins.len()).min_by(|&a, &b| {
        let distance = |i: usize| (pins[i].1[0] - target[0]).powi(2) + (pins[i].1[1] - target[1]).powi(2);
        distance(a).total_cmp(&distance(b)).then(a.cmp(&b))
    }).unwrap();
    pins.remove(index).0
}

fn benchmark_chip_pin(chip: usize, pins: &mut [Vec<(String, [f64; 2])>],
    remaining: &mut [usize], target: [f64; 2]) -> Option<String> {
    if remaining[chip] == 0 { return None; }
    remaining[chip] -= 1;
    Some(nearest_benchmark_pin(&mut pins[chip], target))
}

fn benchmark_200_part_spec(layers: usize) -> Value {
    use forge_server::tools::pcb::footprint;
    use std::collections::{BTreeMap, BTreeSet};
    let mut parts = vec![];
    let mut nets = BTreeMap::<String, Vec<String>>::new();
    let mut chip_pins = vec![];
    let mut sot_pins = vec![];
    let mut passives = vec![];
    let mut positions = BTreeMap::new();
    let mut remaining = vec![];
    let mut centers = vec![];
    let mut seed = 0x7351_b032u64;
    for cluster in 0..20 {
        let at = [-60. + (cluster % 5) as f64 * 30., -37.5 + (cluster / 5) as f64 * 25.];
        centers.push(at);
        let package = ["qfn32", "qfn48", "tssop20", "soic16", "msop10"][cluster % 5];
        let fp = footprint(package).unwrap();
        let reference = format!("U{}", cluster + 1);
        parts.push(json!({"ref":reference,"kind":"chip","footprint":package,"at":at}));
        let mut pins: Vec<_> = fp.pads.iter().map(|p| {
            (format!("{reference}.{}", p.number), [at[0] + p.at[0], at[1] - p.at[1]])
        }).collect();
        positions.extend(pins.iter().cloned());
        let n = if package.starts_with("qfn") { fp.pads.len() - 1 } else { fp.pads.len() };
        let ground = if package.starts_with("qfn") { n + 1 } else { n / 2 };
        for (pin, net) in [(1, "GND"), (ground, "GND"), (n, "3V3")] {
            let name = format!("{reference}.{pin}");
            let index = pins.iter().position(|p| p.0 == name).unwrap();
            pins.remove(index);
            nets.entry(net.into()).or_default().push(name);
        }
        remaining.push(fp.pads.len() * 3 / 4 - 3);
        chip_pins.push(pins);
        let reference = format!("Q{}", cluster + 1);
        let sot_at = [at[0] + 11.8, at[1]];
        parts.push(json!({"ref":reference,"kind":"chip","footprint":"sot23_6","at":sot_at}));
        let pins: Vec<_> = footprint("sot23_6").unwrap().pads.iter().map(|p| {
            (format!("{reference}.{}", p.number), [sot_at[0] + p.at[0], sot_at[1] - p.at[1]])
        }).collect();
        positions.extend(pins.iter().cloned());
        sot_pins.push(pins);
        let mut signal = vec![];
        for k in 0..8 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let decouple = k == 4 || k == 6;
            let radius = if decouple { 6.5 } else { 7.8 + ((seed >> 32) % 7) as f64 / 10. };
            let angle = k as f64 * std::f64::consts::FRAC_PI_4;
            let (sin, cos) = angle.sin_cos();
            let p_at = [at[0] + radius * cos, at[1] + radius * sin];
            let package = if k % 2 == 0 { "0402" } else { "0603" };
            let reference = if k < 4 { format!("R{}_{}", cluster + 1, k + 1) }
                else { format!("C{}_{}", cluster + 1, k - 3) };
            parts.push(json!({"ref":reference,"kind":if k < 4 {"resistor"} else {"capacitor"},
                "footprint":package,"at":p_at,"rotation":angle.to_degrees()}));
            let names = [format!("{reference}.1"), format!("{reference}.2")];
            for pad in footprint(package).unwrap().pads {
                positions.insert(format!("{reference}.{}", pad.number),
                    [p_at[0] + cos * pad.at[0] + sin * pad.at[1], p_at[1] + sin * pad.at[0] - cos * pad.at[1]]);
            }
            if decouple {
                nets.entry("3V3".into()).or_default().push(names[0].clone());
                nets.entry("GND".into()).or_default().push(names[1].clone());
            } else { signal.push(names); }
        }
        passives.push(signal);
    }
    // Reserve long-net pins before local wiring and neighbour buses spend the chip budgets.
    for i in 0..10 {
        let j = 19 - i;
        let a = benchmark_chip_pin(i, &mut chip_pins, &mut remaining, centers[j])
            .unwrap_or_else(|| nearest_benchmark_pin(&mut sot_pins[i], centers[j]));
        let b = benchmark_chip_pin(j, &mut chip_pins, &mut remaining, centers[i])
            .unwrap_or_else(|| nearest_benchmark_pin(&mut sot_pins[j], centers[i]));
        nets.insert(format!("LONG_{i:02}"), vec![a, b]);
    }
    for i in 0..20 {
        let signal = &passives[i];
        for (k, passive) in signal.iter().enumerate() {
            let pin = nearest_benchmark_pin(&mut sot_pins[i], positions[&passive[1]]);
            nets.insert(format!("LOCAL_{i:02}_S{k}"), vec![pin, passive[1].clone()]);
        }
        let local = match i % 5 { 0 | 1 => 4, 4 => 1, _ => 2 };
        if local == 1 {
            for (k, (a, b)) in [(0, 1), (2, 4), (3, 5)].into_iter().enumerate() {
                let mut pins = vec![signal[a][0].clone(), signal[b][0].clone()];
                if k == 0 {
                    pins.push(benchmark_chip_pin(i, &mut chip_pins, &mut remaining, positions[&pins[0]]).unwrap());
                }
                nets.insert(format!("LOCAL_{i:02}_C{k}"), pins);
            }
        } else {
            for k in 0..local {
                let passive = signal[k][0].clone();
                let chip = benchmark_chip_pin(i, &mut chip_pins, &mut remaining, positions[&passive]).unwrap();
                let mut pins = vec![chip, passive];
                if local == 4 && k >= 2 { pins.push(signal[k + 2][0].clone()); }
                nets.insert(format!("LOCAL_{i:02}_C{k}"), pins);
            }
            if local == 2 {
                for (k, (a, b)) in [(2, 4), (3, 5)].into_iter().enumerate() {
                    nets.insert(format!("LOCAL_{i:02}_P{k}"), vec![signal[a][0].clone(), signal[b][0].clone()]);
                }
            }
        }
    }
    let mut bus_bits = 0;
    let mut extras = 0;
    for i in 0..20 {
        for j in [((i % 5) < 4).then_some(i + 1), (i < 15).then_some(i + 5)].into_iter().flatten() {
            let width = 8.min(remaining[i]).min(remaining[j]);
            for bit in 0..width {
                let a = benchmark_chip_pin(i, &mut chip_pins, &mut remaining, centers[j]).unwrap();
                let b = benchmark_chip_pin(j, &mut chip_pins, &mut remaining, centers[i]).unwrap();
                nets.insert(format!("BUS_{i:02}_{j:02}_{bit}"), vec![a, b]);
            }
            bus_bits += width;
            for extra in 0..2.min(remaining[i]).min(remaining[j]) {
                let a = benchmark_chip_pin(i, &mut chip_pins, &mut remaining, centers[j]).unwrap();
                let b = benchmark_chip_pin(j, &mut chip_pins, &mut remaining, centers[i]).unwrap();
                nets.insert(format!("EXTRA_{i:02}_{j:02}_{extra}"), vec![a, b]);
                extras += 1;
            }
        }
    }
    let mut used = BTreeSet::new();
    for pin in nets.values().flatten() { assert!(used.insert(pin.clone()), "pin used twice: {pin}"); }
    let mut chip_used = 0;
    for i in 0..20 {
        let prefix = format!("U{}.", i + 1);
        let count = used.iter().filter(|pin| pin.starts_with(&prefix)).count();
        let pads = footprint(["qfn32", "qfn48", "tssop20", "soic16", "msop10"][i % 5]).unwrap().pads.len();
        assert!(count * 4 <= pads * 3, "U{} exceeds 75% budget", i + 1);
        chip_used += count;
    }
    assert_eq!(parts.len(), 200);
    eprintln!("200-part generator: {} nets, {} connected pins, {chip_used}/512 chip pads used, {bus_bits} bus bits, {extras} extra neighbour nets",
        nets.len(), used.len());
    json!({"board":{"width":if layers == 4 {150} else {180},"height":if layers == 4 {100} else {120},"layers":layers},
        "rules":{"trace":0.15,"clearance":0.15,"via":[0.5,0.25],"edge":0.3,"via_in_pad":true},
        "parts":parts,"nets":nets,"pours":if layers == 4 {
            json!([{"net":"GND","layer":"inner1"},{"net":"3V3","layer":"inner2"}])
        } else { json!([]) }})
}

#[test]
fn benchmark_200_part_generator_is_repeatable_and_respects_pin_budgets() {
    let four = benchmark_200_part_spec(4);
    assert_eq!(four, benchmark_200_part_spec(4));
    let two = benchmark_200_part_spec(2);
    assert_eq!(four["parts"], two["parts"]);
    assert_eq!(four["nets"], two["nets"]);
    let prepared = forge_server::tools::pcb::prepare(&four.to_string()).unwrap();
    assert_eq!(prepared.parts.len(), 200);
    assert_eq!(prepared.nets.len(), four["nets"].as_object().unwrap().len());
}

async fn benchmark_200_part_build(layers: usize) {
    let input = benchmark_200_part_spec(layers);
    let net_count = input["nets"].as_object().unwrap().len();
    let dir = project("route_200", &[("pcb/route200.pcb.json", input.to_string())]);
    let saved = std::env::temp_dir().join("batch3").join(format!("board{layers}"));
    std::fs::create_dir_all(saved.join("pcb")).unwrap();
    std::fs::copy(dir.path.join("pcb/route200.pcb.json"), saved.join("pcb/route200.pcb.json")).unwrap();
    let start = std::time::Instant::now();
    let out = match run("pcb_build", json!({"source":"pcb/route200.pcb.json"}), &dir.path).await {
        Ok(out) => out,
        Err(error) => {
            let text = error.to_string();
            let failed: Vec<_> = text.lines().filter(|l| l.starts_with("unroutable: net ")).collect();
            assert!(!failed.is_empty(), "{text}");
            let routed = net_count - failed.len();
            eprintln!("200-part {layers}L: {routed}/{net_count} nets routed; build {:.3} s\n{text}", start.elapsed().as_secs_f64());
            assert!(layers == 2 && routed * 100 >= net_count * 95, "{text}");
            return;
        }
    };
    let elapsed = start.elapsed();
    assert!(out.text.contains("No errors: every connection is routed."), "{}", out.text);
    for file in &out.files {
        if file.ends_with(".gbr") || file.ends_with("-pcb.svg") || file.ends_with(".circuit.json") {
            let target = saved.join(file);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(dir.path.join(file), target).unwrap();
        }
    }
    let copper: Value = serde_json::from_slice(&std::fs::read(dir.path.join("pcb/route200.circuit.json")).unwrap()).unwrap();
    let vias = copper.as_array().unwrap().iter().filter(|e| e["type"] == "pcb_via").count();
    eprintln!("200-part {layers}L: {net_count}/{net_count} nets, {vias} vias; full build {:.3} s", elapsed.as_secs_f64());
    let start = std::time::Instant::now();
    let drc = run("pcb_drc", json!({"board":"route200"}), &dir.path).await.unwrap();
    let drc_time = start.elapsed();
    eprintln!("200-part {layers}L DRC: {:.3} s\n{}", drc_time.as_secs_f64(), drc.text);
    std::fs::write(saved.join("drc.txt"), &drc.text).unwrap();
    assert!(drc.text.ends_with("No DRC errors."), "{}", drc.text);
    if layers == 4 {
        // shortcut: 20 minute build budget (measured 644 s on 8 threads), tighten when routing gets faster.
        assert!(elapsed.as_secs_f64() < 1200., "build took {elapsed:?}");
        assert!(drc_time.as_secs_f64() < 30., "DRC took {drc_time:?}");
    }
}

#[tokio::test]
#[ignore = "200-part routing benchmark; run in release"]
async fn route_200_part_board_completes_drc_clean() {
    benchmark_200_part_build(4).await;
}

#[tokio::test]
#[ignore = "200-part routing benchmark; run in release"]
async fn two_layer_200_part_board_routes_at_least_95_percent() {
    benchmark_200_part_build(2).await;
}
