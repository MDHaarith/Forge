mod common;
#[path = "common/tools.rs"]
mod support;

use forge_server::tools::{all_tools, describe_tools, is_available};
use forge_server::Settings;
use serde_json::{json, Value};
use support::{project, run};

// Speed of light in vacuum (m/s)
const C0: f64 = 299_792_458.0;

fn dipole_circuit(scale: f64) -> Value {
    let wire = |x| json!({"route_type":"wire","x": x,"y":0,"width": 1.0 * scale,"layer":"top"});
    let board_w = 70.0 * scale;
    let board_h = 10.0 * scale;
    let thick = 0.5 * scale;
    let pad_w = 1.0 * scale;
    let pad_h = 1.0 * scale;
    let gap = 1.5 * scale;
    let p1_x = -gap / 2.0;
    let p2_x = gap / 2.0;
    let arm_end = 29.0 * scale;
    let comp_w = 59.0 * scale;

    json!([
        {"type":"pcb_board","pcb_board_id":"b","center":{"x":0,"y":0},"width": board_w,"height": board_h,"thickness": thick,"num_layers":2},
        {"type":"source_component","source_component_id":"s1","name":"ANT1"},
        {"type":"pcb_component","pcb_component_id":"c1","source_component_id":"s1","center":{"x":0,"y":0},"width": comp_w,"height": pad_h},
        {"type":"pcb_smtpad","pcb_component_id":"c1","shape":"rect","x": p1_x,"y":0,"width": pad_w,"height": pad_h,"layer":"top","port_hints":["pin1"]},
        {"type":"pcb_smtpad","pcb_component_id":"c1","shape":"rect","x": p2_x,"y":0,"width": pad_w,"height": pad_h,"layer":"top","port_hints":["pin2"]},
        {"type":"pcb_trace","pcb_component_id":"c1","route":[wire(p1_x), wire(-arm_end)]},
        {"type":"pcb_trace","pcb_component_id":"c1","route":[wire(p2_x), wire(arm_end)]},
    ])
}

fn fast_dipole_circuit() -> Value {
    let wire = |x| json!({"route_type":"wire","x": x,"y":0,"width": 0.2,"layer":"top"});
    json!([
        {"type":"pcb_board","pcb_board_id":"b","center":{"x":0,"y":0},"width":2,"height":1,"thickness":0.2,"num_layers":2},
        {"type":"source_component","source_component_id":"s1","name":"ANT1"},
        {"type":"pcb_component","pcb_component_id":"c1","source_component_id":"s1","center":{"x":0,"y":0},"width":1.8,"height":0.2},
        {"type":"pcb_smtpad","pcb_component_id":"c1","shape":"rect","x":-0.2,"y":0,"width":0.2,"height":0.2,"layer":"top","port_hints":["pin1"]},
        {"type":"pcb_smtpad","pcb_component_id":"c1","shape":"rect","x":0.2,"y":0,"width":0.2,"height":0.2,"layer":"top","port_hints":["pin2"]},
        {"type":"pcb_trace","pcb_component_id":"c1","route":[wire(-0.2), wire(-0.9)]},
        {"type":"pcb_trace","pcb_component_id":"c1","route":[wire(0.2), wire(0.9)]},
    ])
}

// 1. Tool list: rf_antenna_sim has no requires/install hint for openEMS or python; no tool mentions openEMS.
#[test]
fn test1_tool_list_has_no_openems_or_python_requirements() {
    let dir = common::TempDir::new("tool_list");
    let settings = Settings::new(dir.path.clone(), dir.path.clone());
    let tools = all_tools();

    let rf = tools
        .iter()
        .find(|t| t.name == "rf_antenna_sim")
        .expect("rf_antenna_sim must exist in all_tools()");

    assert!(
        rf.requires.is_empty(),
        "rf_antenna_sim requires must be empty, found: {:?}",
        rf.requires
    );
    assert!(
        rf.needs.is_none(),
        "rf_antenna_sim needs must be None, found: {:?}",
        rf.needs
    );
    assert!(
        rf.install.is_none(),
        "rf_antenna_sim install must be None, found: {:?}",
        rf.install
    );
    assert!(
        is_available(rf, &settings),
        "rf_antenna_sim must be available natively"
    );

    let desc = rf.description.to_lowercase();
    assert!(
        !desc.contains("openems"),
        "rf_antenna_sim description must not mention openEMS: {}",
        rf.description
    );
    assert!(
        !desc.contains("python"),
        "rf_antenna_sim description must not mention python: {}",
        rf.description
    );

    for tool in &tools {
        assert!(
            !tool.name.to_lowercase().contains("openems"),
            "tool {} name mentions openEMS",
            tool.name
        );
        assert!(
            !tool.description.to_lowercase().contains("openems"),
            "tool {} description mentions openEMS: {}",
            tool.name,
            tool.description
        );
        for req in &tool.requires {
            assert!(
                !req.to_lowercase().contains("openems"),
                "tool {} requires mentions openEMS: {req}",
                tool.name
            );
        }
        if let Some(needs) = &tool.needs {
            assert!(
                !needs.to_lowercase().contains("openems"),
                "tool {} needs mentions openEMS: {needs}",
                tool.name
            );
        }
        if let Some(install) = &tool.install {
            for hint in [&install.dnf, &install.flatpak, &install.apt, &install.brew, &install.url] {
                if let Some(text) = hint {
                    assert!(
                        !text.to_lowercase().contains("openems"),
                        "tool {} install hint mentions openEMS: {text}",
                        tool.name
                    );
                }
            }
        }
    }

    let descriptions = describe_tools(&settings);
    for d in &descriptions {
        let name = d["name"].as_str().unwrap_or("");
        let description = d["description"].as_str().unwrap_or("");
        let requires = d.get("requires").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            !description.to_lowercase().contains("openems"),
            "describe_tools for {name} mentions openEMS in description: {description}"
        );
        assert!(
            !requires.to_lowercase().contains("openems"),
            "describe_tools for {name} mentions openEMS in requires: {requires}"
        );
    }
}

// 2. Output files exist; .s1p is valid Touchstone, frequencies increasing, |S11| <= 1 + 1e-6.
//    JSON keys and order as the brief lists. SVGs start with <svg.
#[tokio::test]
async fn test2_output_files_and_formats_are_valid() {
    let circuit = fast_dipole_circuit();
    let dir = project(
        "rf_out_files",
        &[
            (
                "pcb/board.pcb.json",
                serde_json::to_vec(&json!({"board":{"width":2,"height":1}})).unwrap(),
            ),
            (
                "pcb/fast.circuit.json",
                serde_json::to_vec(&circuit).unwrap(),
            ),
        ],
    );

    run("pcb_build", json!({"source": "pcb/board.pcb.json"}), &dir.path)
        .await
        .expect("pcb_build should succeed");

    let out = run(
        "rf_antenna_sim",
        json!({
            "board": "fast",
            "frequency_mhz": 28000,
            "span_mhz": 6000,
            "quality": "fast",
            "er": 1.0,
            "loss_tangent": 0.0
        }),
        &dir.path,
    )
    .await
    .expect("rf_antenna_sim should succeed");

    assert_eq!(
        out.files,
        [
            "rf/fast/ANT1-s11.svg",
            "rf/fast/ANT1-pattern.svg",
            "rf/fast/ANT1.s1p",
            "rf/fast/ANT1.json"
        ]
    );

    for rel in &out.files {
        let p = dir.path.join(rel);
        assert!(p.is_file(), "expected output file {rel} to exist");
    }

    // Check SVGs
    let s11_svg = std::fs::read_to_string(dir.path.join("rf/fast/ANT1-s11.svg")).unwrap();
    assert!(s11_svg.starts_with("<svg"), "S11 SVG must start with <svg");
    assert!(
        s11_svg.trim_end().ends_with("</svg>"),
        "S11 SVG must end with </svg>"
    );

    let pat_svg = std::fs::read_to_string(dir.path.join("rf/fast/ANT1-pattern.svg")).unwrap();
    assert!(pat_svg.starts_with("<svg"), "Pattern SVG must start with <svg");
    assert!(
        pat_svg.trim_end().ends_with("</svg>"),
        "Pattern SVG must end with </svg>"
    );

    // Check Touchstone .s1p
    let s1p_text = std::fs::read_to_string(dir.path.join("rf/fast/ANT1.s1p")).unwrap();
    let mut lines = s1p_text.lines();
    let comment = lines.next().expect("Touchstone comment line");
    assert!(
        comment.starts_with("! Forge FDTD simulation of ANT1"),
        "Touchstone comment line mismatch: {comment}"
    );
    let header = lines.next().expect("Touchstone format line");
    assert_eq!(header, "# Hz S RI R 50", "Touchstone format header mismatch");

    let mut prev_freq = -1.0;
    let mut data_count = 0;
    for line in lines {
        let line = line.trim();
        if line.is_empty() || line.starts_with('!') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(parts.len(), 3, "Touchstone row must have 3 tokens: {line}");
        let freq: f64 = parts[0].parse().expect("frequency in Hz");
        let re: f64 = parts[1].parse().expect("S11 real");
        let im: f64 = parts[2].parse().expect("S11 imag");

        assert!(
            freq > prev_freq,
            "frequencies must strictly increase: {freq} <= {prev_freq}"
        );
        prev_freq = freq;

        let mag_sq = re * re + im * im;
        let mag = mag_sq.sqrt();
        assert!(
            mag <= 1.0 + 1e-6,
            "passive network violation: |S11| = {mag} > 1 + 1e-6 at {freq} Hz"
        );
        data_count += 1;
    }
    assert!(
        data_count >= 201,
        "Touchstone must contain >= 201 frequency points, got {data_count}"
    );

    // Check JSON keys and order
    let json_bytes = std::fs::read(dir.path.join("rf/fast/ANT1.json")).unwrap();
    let res: Value = serde_json::from_slice(&json_bytes).unwrap();
    let obj = res.as_object().expect("root must be a JSON object");
    let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    let expected_keys = [
        "antenna",
        "port",
        "cells",
        "f0",
        "span",
        "resonance",
        "s11_min_db",
        "band",
        "s11_f0_db",
        "zin_f0",
        "zin_res",
        "directivity_dbi",
        "peak_gain_dbi",
        "peak_dir",
        "radiation_efficiency",
        "total_efficiency",
        "ground_under",
        "files",
    ];
    assert_eq!(keys, expected_keys, "JSON keys or key order mismatch");
}

// 3. Determinism: two runs give byte-identical files.
#[tokio::test]
async fn test3_determinism_two_runs_give_byte_identical_files() {
    let circuit = fast_dipole_circuit();
    let sim_args = json!({
        "board": "fast",
        "frequency_mhz": 28000,
        "span_mhz": 6000,
        "quality": "fast",
        "er": 1.0,
        "loss_tangent": 0.0
    });

    let dir1 = project(
        "rf_det1",
        &[
            (
                "pcb/board.pcb.json",
                serde_json::to_vec(&json!({"board":{"width":2,"height":1}})).unwrap(),
            ),
            (
                "pcb/fast.circuit.json",
                serde_json::to_vec(&circuit).unwrap(),
            ),
        ],
    );
    run("pcb_build", json!({"source": "pcb/board.pcb.json"}), &dir1.path)
        .await
        .unwrap();
    run("rf_antenna_sim", sim_args.clone(), &dir1.path)
        .await
        .unwrap();

    let dir2 = project(
        "rf_det2",
        &[
            (
                "pcb/board.pcb.json",
                serde_json::to_vec(&json!({"board":{"width":2,"height":1}})).unwrap(),
            ),
            (
                "pcb/fast.circuit.json",
                serde_json::to_vec(&circuit).unwrap(),
            ),
        ],
    );
    run("pcb_build", json!({"source": "pcb/board.pcb.json"}), &dir2.path)
        .await
        .unwrap();
    run("rf_antenna_sim", sim_args, &dir2.path)
        .await
        .unwrap();

    let filenames = [
        "rf/fast/ANT1-s11.svg",
        "rf/fast/ANT1-pattern.svg",
        "rf/fast/ANT1.s1p",
        "rf/fast/ANT1.json",
    ];

    for file in filenames {
        let bytes1 = std::fs::read(dir1.path.join(file)).expect("file in run 1");
        let bytes2 = std::fs::read(dir2.path.join(file)).expect("file in run 2");
        assert_eq!(
            bytes1, bytes2,
            "non-deterministic output detected in {file}"
        );
    }
}

// 4. Input validation: every error message in the brief, one assertion each.
#[tokio::test]
async fn test4_input_validation_errors() {
    let circuit = dipole_circuit(1.0);
    let dir = project(
        "rf_val",
        &[
            (
                "pcb/board_ok.pcb.json",
                serde_json::to_vec(&json!({"board":{"width":20,"height":10}})).unwrap(),
            ),
            (
                "pcb/ant.circuit.json",
                serde_json::to_vec(&circuit).unwrap(),
            ),
        ],
    );
    run("pcb_build", json!({"source": "pcb/board_ok.pcb.json"}), &dir.path)
        .await
        .unwrap();

    // 1. Invalid board name
    let err = run("rf_antenna_sim", json!({"board": "../ant"}), &dir.path)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("board is the name pcb_build used: letters, digits, - and _"),
        "invalid board name: {err}"
    );

    // 2. Missing board circuit file
    let err = run("rf_antenna_sim", json!({"board": "missing_board"}), &dir.path)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("pcb/missing_board.circuit.json does not exist; build the board with pcb_build first"),
        "missing board: {err}"
    );

    // 3. Antenna name format invalid (spaces/punctuation)
    let err = run(
        "rf_antenna_sim",
        json!({"board": "ant", "antenna": "ANT 1"}),
        &dir.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("antenna is a component name such as ANT1"),
        "invalid antenna name format: {err}"
    );

    // 4. Antenna name not found on board
    let err = run(
        "rf_antenna_sim",
        json!({"board": "ant", "antenna": "ANT_NONEXISTENT"}),
        &dir.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("no component named ANT_NONEXISTENT"),
        "antenna not found: {err}"
    );

    // 5. Frequency out of range
    let err = run(
        "rf_antenna_sim",
        json!({"board": "ant", "frequency_mhz": 40.0}),
        &dir.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("frequency_mhz must be a number from 50 to 30000"),
        "low frequency: {err}"
    );

    // 6. Span out of range
    let err = run(
        "rf_antenna_sim",
        json!({"board": "ant", "frequency_mhz": 2450, "span_mhz": 0.5}),
        &dir.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("span_mhz must be a number from 1 to 30000"),
        "low span: {err}"
    );

    // 7. Span >= 2 * frequency
    let err = run(
        "rf_antenna_sim",
        json!({"board": "ant", "frequency_mhz": 2450, "span_mhz": 5000}),
        &dir.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("span_mhz must be less than twice the frequency"),
        "span >= 2*f0: {err}"
    );

    // 8. Quality invalid
    let err = run(
        "rf_antenna_sim",
        json!({"board": "ant", "quality": "ultra"}),
        &dir.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("quality must be \"fast\" or \"accurate\""),
        "invalid quality: {err}"
    );

    // 9. Permittivity out of range
    let err = run(
        "rf_antenna_sim",
        json!({"board": "ant", "er": 0.5}),
        &dir.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("er must be a number from 1 to 20"),
        "low er: {err}"
    );

    // 10. Loss tangent out of range
    let err = run(
        "rf_antenna_sim",
        json!({"board": "ant", "loss_tangent": -0.01}),
        &dir.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("loss_tangent must be a number from 0 to 0.2"),
        "negative loss tangent: {err}"
    );

    // 11. No antenna found on board
    let circuit_no_ant = json!([
        {"type":"pcb_board","pcb_board_id":"b","center":{"x":0,"y":0},"width":20,"height":10,"thickness":0.5,"num_layers":2}
    ]);
    let dir_no_ant = project(
        "rf_no_ant",
        &[(
            "pcb/ant.circuit.json",
            serde_json::to_vec(&circuit_no_ant).unwrap(),
        )],
    );
    let err = run("rf_antenna_sim", json!({"board": "ant"}), &dir_no_ant.path)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("no antenna found; add an <antenna /> to the board"),
        "no antenna found: {err}"
    );

    // 12. Circuit has no board element
    let circuit_empty = json!([]);
    let dir_empty = project(
        "rf_empty",
        &[(
            "pcb/ant.circuit.json",
            serde_json::to_vec(&circuit_empty).unwrap(),
        )],
    );
    let err = run("rf_antenna_sim", json!({"board": "ant"}), &dir_empty.path)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("the circuit has no board"),
        "circuit has no board: {err}"
    );

    // 13. Single-ended feed without ground reference
    let mut board_single = circuit.clone();
    board_single
        .as_array_mut()
        .unwrap()
        .retain(|e| !(e["type"] == "pcb_smtpad" && e["port_hints"][0] == "pin2"));
    let dir_single = project(
        "rf_single",
        &[(
            "pcb/ant.circuit.json",
            serde_json::to_vec(&board_single).unwrap(),
        )],
    );
    let err = run("rf_antenna_sim", json!({"board": "ant"}), &dir_single.path)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("no ground copper under the antenna feed"),
        "feed with no ground reference: {err}"
    );
}

// 5. Half-wave dipole (ignored, release): resonance within 5 % of c/(2L)·k with your own end-effect estimate,
//    Rin within 15 % of 73 Ω, directivity within 0.5 dB of 2.15 dBi.
//
// Closed-form reference derivations:
// - Physical length L = 59 mm = 0.059 m.
// - Ideal half-wave resonance: f_ideal = c / (2 * L) = 299,792,458 / (2 * 0.059) = 2.540614 GHz.
// - For a flat strip dipole of width w = 1.0 mm, equivalent cylindrical radius a_e = w / 4 = 0.25 mm
//   (Balanis, Antenna Theory 4th ed., Sec 9.7 "Planar Dipole").
// - Length-to-diameter ratio L / (2 * a_e) = 59 / 0.5 = 118.
// - End-effect shortening factor k accounts for fringing capacitance at wire ends and feed gap:
//   For L / (2a) ≈ 100, published thinning/shortening factor is k ≈ 0.94 - 0.96 (Balanis Table 8.1).
//   Using k = 0.940 (corresponding to effective resonant length L ≈ 0.47 * lambda):
//   f_ref = (c / (2 * L)) * k = 2.540614 GHz * 0.940 = 2.388177 GHz.
// - Input resistance Rin of thin half-wave dipole at resonance in free space: Rin_ref = 73.13 ohms.
// - Directivity of thin half-wave dipole: D_max = 1.643 (2.154 dBi).
#[tokio::test]
#[ignore = "half-wave dipole release test; run with cargo test --release -- --ignored"]
async fn test5_half_wave_dipole_resonance_and_radiation() {
    let circuit = dipole_circuit(1.0);
    let dir = project(
        "rf_dipole",
        &[
            (
                "pcb/board.pcb.json",
                serde_json::to_vec(&json!({"board":{"width":70,"height":10}})).unwrap(),
            ),
            (
                "pcb/ant.circuit.json",
                serde_json::to_vec(&circuit).unwrap(),
            ),
        ],
    );
    run("pcb_build", json!({"source": "pcb/board.pcb.json"}), &dir.path)
        .await
        .unwrap();

    let started = std::time::Instant::now();
    let out = run(
        "rf_antenna_sim",
        json!({
            "board": "ant",
            "frequency_mhz": 2450,
            "span_mhz": 1600,
            "quality": "fast",
            "er": 1.0,
            "loss_tangent": 0.0
        }),
        &dir.path,
    )
    .await
    .expect("rf_antenna_sim dipole run");

    let duration_s = started.elapsed().as_secs_f64();
    let json_bytes = std::fs::read(dir.path.join("rf/ant/ANT1.json")).unwrap();
    let res: Value = serde_json::from_slice(&json_bytes).unwrap();

    let resonance_hz = res["resonance"].as_f64().unwrap();
    let rin_ohms = res["zin_res"][0].as_f64().unwrap();
    let directivity_dbi = res["directivity_dbi"].as_f64().unwrap();
    let efficiency = res["radiation_efficiency"].as_f64().unwrap();

    // Closed-form math:
    // L = 59 mm, strip width w = 1 mm, equivalent radius a = 0.25 mm
    let l_m = 0.059;
    let f_ideal = C0 / (2.0 * l_m); // 2.540614e9 Hz
    let k_end_effect = 0.940; // 0.47 lambda shortening factor for planar strip with L/2a ≈ 118
    let f_ref = f_ideal * k_end_effect; // ~ 2.388177 GHz
    let r_ref = 73.13; // 73.13 ohms standard thin dipole input resistance
    let d_ref_dbi = 2.154; // 10 * log10(1.643)

    let f_err = (resonance_hz - f_ref).abs() / f_ref;
    let r_err = (rin_ohms - r_ref).abs() / r_ref;
    let d_err = (directivity_dbi - d_ref_dbi).abs();

    eprintln!(
        "TEST 5 DIPOLE: measured f={:.3} MHz (ref={:.3} MHz, err={:.2}%), Rin={:.2} ohm (ref={:.2} ohm, err={:.2}%), D={:.3} dBi (ref={:.3} dBi, err={:.3} dB), eff={:.3}, runtime={:.2}s",
        resonance_hz / 1e6,
        f_ref / 1e6,
        f_err * 100.0,
        rin_ohms,
        r_ref,
        r_err * 100.0,
        directivity_dbi,
        d_ref_dbi,
        d_err,
        efficiency,
        duration_s
    );

    assert!(
        f_err <= 0.05,
        "resonance {resonance_hz} Hz not within 5% of reference {f_ref} Hz (err={:.2}%)",
        f_err * 100.0
    );
    assert!(
        r_err <= 0.15,
        "Rin {rin_ohms} ohm not within 15% of reference {r_ref} ohm (err={:.2}%)",
        r_err * 100.0
    );
    assert!(
        d_err <= 0.5,
        "directivity {directivity_dbi} dBi not within 0.5 dB of reference {d_ref_dbi} dBi (err={:.3} dB)",
        d_err
    );
    assert!(
        efficiency > 0.90 && efficiency < 1.10,
        "efficiency {efficiency} should be ~1.0 in free space"
    );
    assert_eq!(out.files.len(), 4);
}

// 6. Scaling (ignored, release): halving every dimension of the dipole doubles the resonant frequency within 3 %.
//
// Closed-form scaling rationale:
// - Under Maxwell's equations in non-dispersive media (vacuum er=1, tand=0), spatial scaling x' = alpha * x
//   with alpha = 0.5 scales all resonant frequencies by f' = f / alpha = 2.0 * f.
// - Halving length, width, thickness, and feed gap by 0.5 doubles the resonant frequency within 3 %.
#[tokio::test]
#[ignore = "scaling release test; run with cargo test --release -- --ignored"]
async fn test6_dipole_dimensional_scaling_doubles_frequency() {
    // 1. Full-size dipole (L = 59 mm, f0 = 2450 MHz, span = 1600 MHz)
    let circuit1 = dipole_circuit(1.0);
    let dir1 = project(
        "rf_scale1",
        &[
            (
                "pcb/board.pcb.json",
                serde_json::to_vec(&json!({"board":{"width":70,"height":10}})).unwrap(),
            ),
            (
                "pcb/ant.circuit.json",
                serde_json::to_vec(&circuit1).unwrap(),
            ),
        ],
    );
    run("pcb_build", json!({"source": "pcb/board.pcb.json"}), &dir1.path)
        .await
        .unwrap();

    let started1 = std::time::Instant::now();
    run(
        "rf_antenna_sim",
        json!({
            "board": "ant",
            "frequency_mhz": 2450,
            "span_mhz": 1600,
            "quality": "fast",
            "er": 1.0,
            "loss_tangent": 0.0
        }),
        &dir1.path,
    )
    .await
    .expect("full-scale dipole run");
    let dur1 = started1.elapsed().as_secs_f64();

    let res1: Value = serde_json::from_slice(
        &std::fs::read(dir1.path.join("rf/ant/ANT1.json")).unwrap(),
    )
    .unwrap();
    let f1 = res1["resonance"].as_f64().unwrap();

    // 2. Half-size dipole (L = 29.5 mm, f0 = 4900 MHz, span = 3200 MHz)
    let circuit2 = dipole_circuit(0.5);
    let dir2 = project(
        "rf_scale2",
        &[
            (
                "pcb/board.pcb.json",
                serde_json::to_vec(&json!({"board":{"width":35,"height":5}})).unwrap(),
            ),
            (
                "pcb/ant.circuit.json",
                serde_json::to_vec(&circuit2).unwrap(),
            ),
        ],
    );
    run("pcb_build", json!({"source": "pcb/board.pcb.json"}), &dir2.path)
        .await
        .unwrap();

    let started2 = std::time::Instant::now();
    run(
        "rf_antenna_sim",
        json!({
            "board": "ant",
            "frequency_mhz": 4900,
            "span_mhz": 3200,
            "quality": "fast",
            "er": 1.0,
            "loss_tangent": 0.0
        }),
        &dir2.path,
    )
    .await
    .expect("half-scale dipole run");
    let dur2 = started2.elapsed().as_secs_f64();

    let res2: Value = serde_json::from_slice(
        &std::fs::read(dir2.path.join("rf/ant/ANT1.json")).unwrap(),
    )
    .unwrap();
    let f2 = res2["resonance"].as_f64().unwrap();

    // Halving dimensions should double the frequency: f2 ≈ 2 * f1 within 3%
    let ratio = f2 / f1;
    let error = (ratio / 2.0 - 1.0).abs();

    eprintln!(
        "TEST 6 SCALING: f1(59mm)={:.3} MHz ({:.2}s), f2(29.5mm)={:.3} MHz ({:.2}s), ratio={:.5} (target=2.0, error={:.2}%)",
        f1 / 1e6,
        dur1,
        f2 / 1e6,
        dur2,
        ratio,
        error * 100.0
    );

    assert!(
        error <= 0.03,
        "halving dimensions did not double resonant frequency within 3%: f1={f1}, f2={f2}, ratio={ratio}, error={:.2}%",
        error * 100.0
    );
}
