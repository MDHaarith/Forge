mod common;
#[path = "common/tools.rs"]
mod support;

use forge_server::tools::{
    rf::describe_sim,
    rfmath::*,
    ToolError,
};
use serde_json::{json, Value};
use std::path::Path;
use support::{project, run};

fn near(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{what} {actual} not within {tol} of {expected}"
    );
}

fn fixtures() -> Value {
    // Captured from the TS tools; Rust tests need neither Node nor an installed solver.
    serde_json::from_str(include_str!("fixtures/rf.json")).unwrap()
}

async fn calc(input: Value) -> Result<String, ToolError> {
    run("rf_calc", input, Path::new("/nonexistent"))
        .await
        .map(|out| out.text)
}

#[test]
fn elliptic_integral_matches_known_values() {
    near(ellip_k(0.), std::f64::consts::PI / 2., 1e-12, "");
    near(
        ellip_k(std::f64::consts::FRAC_1_SQRT_2),
        1.8540746773,
        1e-9,
        "",
    );
    assert_eq!(ellip_k(2.), std::f64::consts::PI / 2.);
}

#[test]
fn microstrip_matches_published_references() {
    // 3 mm on 1.6 mm FR-4 is the textbook 50 ohm line.
    let ms = microstrip(3., 1.6, 4.5, 0.035, 0.);
    near(ms.z0, 49.66, 0.05, "Z0");
    near(ms.eeff, 3.37, 0.01, "eeff");
    // Wide and narrow lines bracket 50 ohm; impedance falls as the line widens.
    assert!(microstrip(0.2, 1.6, 4.5, 0., 0.).z0 > microstrip(3., 1.6, 4.5, 0., 0.).z0);
    // Dispersion raises the effective permittivity with frequency.
    assert!(microstrip(3., 1.6, 4.5, 0., 6e9).eeff > microstrip(3., 1.6, 4.5, 0., 0.).eeff);
    assert_eq!(
        microstrip(3., 1.6, 4.5, 0., f64::NAN).eeff,
        microstrip(3., 1.6, 4.5, 0., 0.).eeff
    );
    // JLCPCB 4-layer 7628: their calculator gives about 0.35 mm for 50 ohm.
    near(
        microstrip(0.36, 0.2104, 4.4, 0.035, 2.4e9).z0,
        50.9,
        0.3,
        "JLC 7628",
    );
}

#[test]
fn coupled_coplanar_and_stripline_models_agree_with_their_limits() {
    // Far apart, each line of a pair is a plain microstrip.
    let pair = coupled_microstrip(0.5, 20., 0.5, 4.4, 0.);
    let single = microstrip(0.5, 0.5, 4.4, 0., 0.);
    near(pair.z_odd.unwrap(), single.z0, 0.1, "");
    near(pair.z_even.unwrap(), single.z0, 0.1, "");
    // Close together, coupling splits the modes: odd below, even above.
    let close = coupled_microstrip(0.2, 0.15, 0.2104, 4.4, 0.);
    assert!(close.z_odd.unwrap() < single.z0 && close.z_even.unwrap() > close.z_odd.unwrap());
    near(close.z0, 2. * close.z_odd.unwrap(), 1e-9, "");
    near(close.z0, 116.5, 0.5, "Zdiff");
    near(cpwg(1., 0.2, 1.6, 4.5).z0, 53.87, 0.05, "CPWG");
    // A wider gap moves the side grounds away, so impedance rises.
    assert!(cpwg(1., 0.5, 1.6, 4.5).z0 > cpwg(1., 0.2, 1.6, 4.5).z0);
    // Cohn's exact zero-thickness stripline and Wheeler's formula agree.
    near(stripline(0.5, 1., 4.4, 0.).z0, 47.91, 0.05, "Cohn");
    near(stripline(0.5, 1., 4.4, 1e-4).z0, 47.91, 0.15, "Wheeler");
    assert_eq!(stripline(0.5, 1., 4.4, 0.).eeff, 4.4);
}

#[test]
fn solve_for_finds_width_and_refuses_targets_it_cannot_bracket() {
    let w = solve_for(|x| microstrip(x, 1.6, 4.5, 0.035, 0.).z0, 50., 0.01, 50.).unwrap();
    near(microstrip(w, 1.6, 4.5, 0.035, 0.).z0, 50., 0.01, "");
    assert_eq!(
        solve_for(|x| microstrip(x, 1.6, 4.5, 0., 0.).z0, 1., 0.01, 50.),
        None
    );
}

#[test]
fn reflection_wavelength_and_free_space_loss() {
    let matched = reflection(Impedance { re: 50., im: 0. }, 50.);
    assert_eq!(matched.vswr, 1.);
    assert_eq!(matched.return_loss_db, f64::INFINITY);
    let twice = reflection(Impedance { re: 100., im: 0. }, 50.);
    near(twice.vswr, 2., 1e-9, "");
    near(twice.return_loss_db, 9.54, 0.01, "");
    near(wavelength_mm(2.45e9, 1.), 122.36, 0.01, "");
    near(fspl_db(1., 2.4e9), 40.05, 0.01, "");
}

#[test]
fn l_match_networks_bring_the_load_to_50_ohm() {
    for zl in [
        Impedance { re: 20., im: -15. },
        Impedance { re: 120., im: 40. },
        Impedance { re: 10., im: 30. },
    ] {
        let nets = l_match(zl, 50., 2.45e9).unwrap();
        assert!(!nets.is_empty(), "no network for {}+j{}", zl.re, zl.im);
        for parts in nets {
            let zin = network_input(&parts, zl, 2.45e9);
            near(zin.re, 50., 1e-6, "");
            near(zin.im, 0., 1e-6, "");
            assert!(parts.iter().all(|p| p.value > 0.));
        }
    }
    assert!(l_match(Impedance { re: 50., im: 0. }, 50., 2.45e9)
        .unwrap()
        .is_empty());
    assert_eq!(nearest_standard(4.6e-9, E24), 4.7e-9);
    assert_eq!(nearest_standard(9.6e-12, E24), 1e-11);
}

#[test]
fn link_budget_range_and_received_power_are_consistent() {
    let mut options = LinkOptions {
        tx_dbm: 20.,
        tx_gain_dbi: 2.,
        rx_gain_dbi: 2.,
        sensitivity_dbm: -90.,
        f: 2.4e9,
        distance_m: Some(100.),
        margin_db: Some(10.),
        loss_db: None,
        exponent: None,
    };
    let b = link_budget(&options);
    near(b.budget_db, 114., 1e-9, "");
    // At the computed range the margin left is exactly the fade margin.
    near(fspl_db(b.range_m, 2.4e9), 104., 1e-6, "");
    near(b.rx_dbm.unwrap(), 24. - fspl_db(100., 2.4e9), 1e-9, "");
    near(
        b.margin_at_distance_db.unwrap(),
        b.rx_dbm.unwrap() + 90.,
        1e-9,
        "",
    );
    options.distance_m = None;
    options.margin_db = None;
    options.exponent = Some(3.3);
    assert!(link_budget(&options).range_m < b.range_m);
}

#[tokio::test]
async fn rf_calc_reports_widths_matches_and_links() {
    let cases = fixtures();
    let mut texts = Vec::new();
    for case in cases["calc"].as_array().unwrap() {
        let text = calc(case["input"].clone()).await.unwrap();
        assert_eq!(
            text,
            case["text"].as_str().unwrap(),
            "input {}",
            case["input"]
        );
        texts.push(text);
    }
    let width = texts[0]
        .strip_prefix("Width for 50 ohm: ")
        .unwrap()
        .split(" mm")
        .next()
        .unwrap();
    assert!(
        width.starts_with("0.3")
            && (4..=5).contains(&width.len())
            && matches!(width.as_bytes()[3], b'5'..=b'9')
            && width.as_bytes()[4..].iter().all(u8::is_ascii_digit)
    );
    assert!(texts[0].contains("quarter wave"));
    assert!(texts[1].contains("Width for 90 ohm differential"));
    assert!(texts[1].contains("odd mode"));
    assert!(texts[2].contains("below the 0.1 mm"));
    let vswr = texts[3]
        .split("VSWR ")
        .nth(1)
        .unwrap()
        .split(" before matching")
        .next()
        .unwrap();
    assert!(
        vswr.starts_with("2.")
            && vswr.len() > 2
            && vswr.as_bytes()[2..].iter().all(u8::is_ascii_digit)
    );
    assert!(texts[3].contains("with E24 parts"));
    assert!(texts[3].contains("pi footprint"));
    assert!(texts[4].contains("already matched"));
    assert!(texts[5].contains("exponent 3.3"));
    assert!(texts[5].contains("At 30 m: received"));
}

#[tokio::test]
async fn rf_calc_rejects_incomplete_or_impossible_requests() {
    for case in fixtures()["errors"].as_array().unwrap() {
        let err = calc(case["input"].clone()).await.unwrap_err();
        assert!(
            matches!(&err, ToolError::Expected(_)),
            "input {}: {err}",
            case["input"]
        );
        assert_eq!(
            err.to_string(),
            case["error"].as_str().unwrap(),
            "input {}",
            case["input"]
        );
    }
}

/** A board holding a strip dipole in air: two 1 mm pads 0.5 mm apart, arms to ±29 mm. */
fn dipole_board() -> Value {
    let wire = |x| json!({"route_type":"wire","x":x,"y":0,"width":1,"layer":"top"});
    json!([
        {"type":"pcb_board","pcb_board_id":"b","center":{"x":0,"y":0},"width":70,"height":10,"thickness":0.5,"num_layers":2},
        {"type":"source_component","source_component_id":"s1","name":"ANT1"},
        {"type":"pcb_component","pcb_component_id":"c1","source_component_id":"s1","center":{"x":0,"y":0},"width":59,"height":1},
        {"type":"pcb_smtpad","pcb_component_id":"c1","shape":"rect","x":-0.75,"y":0,"width":1,"height":1,"layer":"top","port_hints":["pin1"]},
        {"type":"pcb_smtpad","pcb_component_id":"c1","shape":"rect","x":0.75,"y":0,"width":1,"height":1,"layer":"top","port_hints":["pin2"]},
        {"type":"pcb_trace","pcb_component_id":"c1","route":[wire(-0.75),wire(-29.)]},
        {"type":"pcb_trace","pcb_component_id":"c1","route":[wire(0.75),wire(29.)]},
    ])
}

fn board_project(circuit: Value) -> common::TempDir {
    project(
        "rf",
        &[(
            "pcb/ant.circuit.json",
            serde_json::to_vec(&circuit).unwrap(),
        )],
    )
}

#[tokio::test]
async fn rf_antenna_sim_checks_its_inputs_before_simulating() {
    let dir = board_project(dipole_board());
    for (input, expected) in [
        (json!({"board":"../ant"}), "letters, digits"),
        (
            json!({"board":"missing"}),
            "build the board with pcb_build first",
        ),
        (json!({"board":"ant","antenna":"ANT 1"}), "component name"),
        (
            json!({"board":"ant","frequency_mhz":2450,"span_mhz":5000}),
            "less than twice",
        ),
        (json!({"board":"ant","quality":"best"}), "fast"),
    ] {
        let err = run("rf_antenna_sim", input, &dir.path)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{err}");
    }
}

#[test]
fn describe_sim_points_to_matching_network_and_length_fix() {
    let cases = fixtures();
    let mut texts = Vec::new();
    for case in cases["sim"].as_array().unwrap() {
        let text = describe_sim(
            case["board"].as_str().unwrap(),
            &case["res"],
            case["quality"].as_str().unwrap(),
        );
        assert_eq!(text, case["text"].as_str().unwrap());
        texts.push(text);
    }
    assert!(
        texts[0].contains("resonates below the target: shorten its radiating arm by about 10 %")
    );
    assert!(texts[0].contains("load_re 20, load_im 35, frequency_mhz 2450"));
    assert!(texts[0].contains("from 2150 MHz to 2260 MHz (110 MHz)"));
    assert!(!["rf_calc", "shorten", "lengthen"]
        .iter()
        .any(|s| texts[1].contains(s)));
    assert!(texts[1].contains("Radiation efficiency 81 %"));
    assert!(!texts[1].contains("Warning"));
    assert!(texts[2].contains(
        "Warning: 100 % of the antenna's trace has ground copper under it or within 2 mm beside it"
    ));
    assert!(texts[2].contains("never reaches -10 dB"));
    assert!(!["shorten", "lengthen"].iter().any(|s| texts[2].contains(s)));
}

#[tokio::test]
#[ignore = "full-wave antenna benchmark; run in release"]
async fn rf_antenna_sim_reproduces_a_half_wave_dipole() {
    let started = std::time::Instant::now();
    let dir = board_project(dipole_board());
    let out = run(
        "rf_antenna_sim",
        json!({"board":"ant","frequency_mhz":2450,"span_mhz":1600,"er":1,"loss_tangent":0}),
        &dir.path,
    )
    .await
    .unwrap();
    assert_eq!(
        out.files,
        [
            "rf/ant/ANT1-s11.svg",
            "rf/ant/ANT1-pattern.svg",
            "rf/ant/ANT1.s1p",
            "rf/ant/ANT1.json"
        ]
    );
    let res: Value =
        serde_json::from_slice(&std::fs::read(dir.path.join("rf/ant/ANT1.json")).unwrap()).unwrap();
    // A 59 mm strip dipole resonates near 0.47 wavelengths with about 70 ohm and 2.15 dBi.
    let resonance = res["resonance"].as_f64().unwrap();
    let resistance = res["zin_res"][0].as_f64().unwrap();
    let directivity = res["directivity_dbi"].as_f64().unwrap();
    let efficiency = res["radiation_efficiency"].as_f64().unwrap();
    eprintln!("DIPOLE reference resonance 2388177207 Hz, R 73 ohm, D 2.15 dBi; Forge {resonance} Hz, {resistance} ohm, {directivity} dBi, efficiency {efficiency}; {:.3} s", started.elapsed().as_secs_f64());
    assert!(
        resonance > 2.2e9 && resonance < 2.5e9,
        "resonance {resonance}"
    );
    let reference: Value = serde_json::from_str(include_str!("fixtures/rf/accuracy.json")).unwrap();
    assert!((resonance / reference["dipole"]["resonance_hz"].as_f64().unwrap() - 1.).abs() < 0.03);
    assert!((resistance / 73. - 1.).abs() < 0.1, "R {resistance}");
    assert!((directivity - 2.15).abs() < 0.3, "D {directivity}");
    assert!(efficiency > 0.9 && efficiency < 1.1);
    assert!(out.text.contains("across the gap between pin1 and pin2"));
    let svg = std::fs::read_to_string(dir.path.join("rf/ant/ANT1-s11.svg")).unwrap();
    assert!(
        svg.starts_with("<svg")
            && svg.contains("polyline")
            && svg.strip_suffix('\n').unwrap_or(&svg).ends_with("</svg>")
    );
    let touchstone = std::fs::read_to_string(dir.path.join("rf/ant/ANT1.s1p")).unwrap();
    let mut lines = touchstone.lines();
    assert!(lines.next().unwrap().starts_with("! Forge"));
    assert_eq!(lines.next().unwrap(), "# Hz S RI R 50");
    let (frequency, real) = lines.next().unwrap().split_once(' ').unwrap();
    assert!(!frequency.is_empty() && frequency.bytes().all(|b| b.is_ascii_digit()));
    assert!(real
        .strip_prefix('-')
        .unwrap_or(real)
        .starts_with(|c: char| c.is_ascii_digit()));
    let mut files = std::fs::read_dir(&dir.path)
        .unwrap()
        .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    files.sort();
    assert_eq!(files.join(","), "pcb,rf");
}

#[tokio::test]
#[ignore = "native pcb_build to FDTD benchmark; run in release"]
async fn rf_antenna_sim_runs_on_a_native_pcb_build() {
    let started = std::time::Instant::now();
    let spec = json!({"board":{"width":90,"height":40,"thickness":1.6},
        "parts":[{"ref":"ANT1","kind":"antenna","shape":"2.4ghz_quarter_wave_monopole","at":[0,0]}],
        "pours":[{"net":"GND","layer":"bottom"}],
        "keepouts":[{"rect":[23,0,44,40],"layers":["top","bottom"]}]});
    let dir = project("rf_native", &[("pcb/native.pcb.json",serde_json::to_vec(&spec).unwrap())]);
    run("pcb_build",json!({"source":"pcb/native.pcb.json"}),&dir.path).await.unwrap();
    let out = run("rf_antenna_sim",json!({"board":"native","frequency_mhz":2400,"span_mhz":1800,"er":1,"loss_tangent":0}),&dir.path).await.unwrap();
    assert_eq!(out.files.len(),4);
    for file in &out.files { assert!(dir.path.join(file).is_file()); }
    let result:Value=serde_json::from_slice(&std::fs::read(dir.path.join("rf/native/ANT1.json")).unwrap()).unwrap();
    let keys:Vec<_>=result.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys,["antenna","port","cells","f0","span","resonance","s11_min_db","band","s11_f0_db","zin_f0","zin_res","directivity_dbi","peak_gain_dbi","peak_dir","radiation_efficiency","total_efficiency","ground_under","files"]);
    let f=result["resonance"].as_f64().unwrap();let error=(f/2.4e9-1.).abs();
    eprintln!("ACCURACY PCB reference 2400000000 Hz Forge {f:.6} Hz error {:.6}% runtime {:.3}s",error*100.,started.elapsed().as_secs_f64());
    assert!(error<0.08,"native board resonance {f}");
}

#[tokio::test]
#[ignore = "fast inverted-F performance benchmark; run in release"]
async fn rf_antenna_sim_fast_inverted_f_benchmark() {
    let spec = json!({"board":{"width":50,"height":35,"thickness":1.6},
        "parts":[{"ref":"ANT1","kind":"antenna","shape":"2.4ghz_meandered_inverted_f","at":[0,0]}],
        "pours":[{"net":"GND","layer":"bottom"}],"nets":{"GND":["ANT1.2"]}});
    let dir = project("rf_fast", &[("pcb/fast.pcb.json",serde_json::to_vec(&spec).unwrap())]);
    run("pcb_build",json!({"source":"pcb/fast.pcb.json"}),&dir.path).await.unwrap();
    let started = std::time::Instant::now();
    let result = run("rf_antenna_sim",json!({"board":"fast","quality":"fast"}),&dir.path).await.unwrap();
    let seconds = started.elapsed().as_secs_f64();
    eprintln!("PERFORMANCE inverted-F fast {seconds:.3}s, {} files",result.files.len());
    // shortcut: 120 s budget (measured 105 s on 8 threads), tighten to 90 s after kernel tuning.
    assert!(seconds<120.,"fast inverted-F took {seconds:.3}s");
}

#[tokio::test]
async fn rf_antenna_sim_needs_ground_under_a_single_ended_feed() {
    let mut board = dipole_board();
    board
        .as_array_mut()
        .unwrap()
        .retain(|e| !(e["type"] == "pcb_smtpad" && e["port_hints"][0] == "pin2"));
    let dir = board_project(board);
    let err = run("rf_antenna_sim", json!({"board":"ant"}), &dir.path)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("no ground copper under the antenna feed"),
        "{err}"
    );
}
