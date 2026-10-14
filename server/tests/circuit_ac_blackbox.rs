mod common;
#[path = "common/tools.rs"]
mod support;

use common::TempDir;
use serde_json::json;
use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::fs;
use std::path::Path;

/// Physical constants specified in contract:
/// k = 1.380649e-23 J/K, q = 1.602176634e-19 C, T_kelvin = T_celsius + 273.15
const BOLTZMANN_K: f64 = 1.380649e-23;
const ELECTRON_Q: f64 = 1.602176634e-19;

fn to_kelvin(celsius: f64) -> f64 {
    celsius + 273.15
}

/// Thermal voltage Vt = k * T / q
fn thermal_voltage(celsius: f64) -> f64 {
    BOLTZMANN_K * to_kelvin(celsius) / ELECTRON_Q
}

async fn simulate(name: &str, deck: &str) -> (TempDir, forge_server::tools::ToolResult) {
    let rel_path = format!("sim/{name}.cir");
    let dir = support::project("bb_ac_sim", &[(&rel_path, deck)]);
    let res = support::run("circuit_simulate", json!({"source": rel_path}), &dir.path)
        .await
        .expect("circuit_simulate should succeed");
    (dir, res)
}

async fn simulate_err(name: &str, deck: &str) -> (TempDir, String) {
    let rel_path = format!("sim/{name}.cir");
    let dir = support::project("bb_ac_err", &[(&rel_path, deck)]);
    let err = support::run("circuit_simulate", json!({"source": rel_path}), &dir.path)
        .await
        .expect_err("circuit_simulate should fail");
    (dir, err.to_string())
}

fn parse_op_file(path: &Path) -> BTreeMap<String, f64> {
    let content = fs::read_to_string(path).expect("read -op.txt");
    content
        .lines()
        .filter_map(|line| {
            let (label, rest) = line.split_once(" = ")?;
            let val_str = rest.split_whitespace().next()?;
            let val = val_str.parse::<f64>().ok()?;
            Some((label.to_ascii_lowercase(), val))
        })
        .collect()
}

fn parse_csv_file(path: &Path) -> (Vec<String>, Vec<Vec<f64>>) {
    let content = fs::read_to_string(path).expect("read csv file");
    let mut lines = content.lines();
    let header_line = lines.next().expect("csv header");
    let headers: Vec<String> = header_line
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    let rows: Vec<Vec<f64>> = lines
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            line.split(',')
                .map(|v| v.trim().parse::<f64>().expect("valid f64 in csv"))
                .collect()
        })
        .collect();
    (headers, rows)
}

/// 1. RC low-pass `.ac dec 20 1 1meg`:
/// |H| = 1/√(1+(ωRC)²) and phase = −atan(ωRC) at every row (1e-6 rel, 1e-4°);
/// −3 dB point at 1/(2πRC) within 1 %; frequency column log-spaced with 20 points/decade.
#[tokio::test]
async fn test_1_rc_lowpass_ac_dec_bode_and_cutoff() {
    // RC low-pass circuit:
    // Input AC voltage source V1 driving series resistor R1 and shunt capacitor C1 to ground.
    // Transfer function H(jω) = 1 / (1 + j*ω*R*C)
    // Magnitude: |H(ω)| = 1 / sqrt(1 + (ω*R*C)^2)
    // Phase: ∠H(ω) = -atan(ω*R*C) in radians, or -atan(ω*R*C) * (180 / π) in degrees.
    // Cutoff (-3 dB) frequency: f_c = 1 / (2 * π * R * C)
    let r: f64 = 1000.0; // 1 kΩ
    let c: f64 = 1.0e-7; // 100 nF = 0.1 µF

    let deck = format!(
        "RC Low-Pass Filter\n\
         V1 in 0 AC 1\n\
         R1 in out {r}\n\
         C1 out 0 {c}\n\
         .ac dec 20 1 1meg\n\
         .end\n"
    );

    let (dir, _) = simulate("rc_lp", &deck).await;
    let (headers, rows) = parse_csv_file(&dir.path.join("sim/rc_lp/rc_lp-ac.csv"));

    let freq_col = headers.iter().position(|h| h == "freq").unwrap();
    let vm_out_col = headers.iter().position(|h| h == "vm(out)").unwrap();
    let vp_out_col = headers.iter().position(|h| h == "vp(out)").unwrap();

    // From 1 Hz to 1 MHz (1e6 Hz) with 20 points/decade:
    // Total decades = log10(1e6) - log10(1) = 6 decades.
    // Total points = 6 * 20 + 1 = 121 points.
    assert_eq!(rows.len(), 121, "expected 121 points for 6 decades at 20 pts/decade");

    let f_cutoff_theoretical = 1.0 / (2.0 * PI * r * c);

    for (i, row) in rows.iter().enumerate() {
        let f = row[freq_col];
        // Theoretical frequency for dec sweep: f_i = 10^(i / 20)
        let f_expected = 10.0f64.powf(i as f64 / 20.0);
        let f_rel_err = (f - f_expected).abs() / f_expected;
        assert!(
            f_rel_err < 1e-6,
            "row {i}: frequency {f} not log-spaced (expected {f_expected}, rel err {f_rel_err})"
        );

        // Angular frequency ω = 2 * π * f
        let omega = 2.0 * PI * f;
        let omega_rc = omega * r * c;

        // Theoretical closed-form magnitude: |H| = 1 / sqrt(1 + (ω*R*C)^2)
        let mag_theoretical = 1.0 / (1.0 + omega_rc * omega_rc).sqrt();
        let mag_sim = row[vm_out_col];
        let mag_rel_err = (mag_sim - mag_theoretical).abs() / mag_theoretical;
        assert!(
            mag_rel_err < 1e-6,
            "row {i} at {f} Hz: mag {mag_sim} vs theoretical {mag_theoretical} (rel err {mag_rel_err} > 1e-6)"
        );

        // Theoretical closed-form phase: phase = -atan(ω*R*C) [converted to degrees]
        let phase_theoretical = -omega_rc.atan() * (180.0 / PI);
        let phase_sim = row[vp_out_col];
        let phase_abs_err = (phase_sim - phase_theoretical).abs();
        assert!(
            phase_abs_err < 1e-4,
            "row {i} at {f} Hz: phase {phase_sim}° vs theoretical {phase_theoretical}° (abs err {phase_abs_err}° > 1e-4°)"
        );
    }

    // −3 dB point corresponds to |H| = 1 / sqrt(2) ≈ 0.70710678.
    // Interpolate in log-frequency space where magnitude crosses 1 / sqrt(2):
    let target_mag = 1.0 / 2.0f64.sqrt();
    let cross_idx = rows
        .iter()
        .position(|r| r[vm_out_col] <= target_mag)
        .expect("must cross -3 dB point");
    assert!(cross_idx > 0, "cross index must be after row 0");

    let r0 = &rows[cross_idx - 1];
    let r1 = &rows[cross_idx];
    let m0 = r0[vm_out_col];
    let m1 = r1[vm_out_col];
    let ln_f0 = r0[freq_col].ln();
    let ln_f1 = r1[freq_col].ln();
    let alpha = (target_mag - m0) / (m1 - m0);
    let ln_fc_meas = ln_f0 + alpha * (ln_f1 - ln_f0);
    let fc_measured = ln_fc_meas.exp();

    let cutoff_rel_err = (fc_measured - f_cutoff_theoretical).abs() / f_cutoff_theoretical;
    assert!(
        cutoff_rel_err < 0.01,
        "measured cutoff {fc_measured} Hz vs theoretical {f_cutoff_theoretical} Hz (rel err {cutoff_rel_err} > 1%)"
    );
}

/// 2. Series RLC band-pass:
/// peak at 1/(2π√LC) within 0.5 %, Q from −3 dB bandwidth = (1/R)√(L/C) within 2 %.
#[tokio::test]
async fn test_2_series_rlc_bandpass_peak_and_q_bandwidth() {
    // Series RLC circuit:
    // Voltage source V1 driving series L, series C, and resistor R to ground.
    // Output voltage taken across R.
    // Transfer function: H(jω) = R / (R + j*(ωL - 1/(ωC))) = 1 / (1 + j*(ωL - 1/(ωC))/R)
    // Resonance frequency: ω0 = 1 / sqrt(L * C)  =>  f0 = 1 / (2 * π * sqrt(L * C))
    // At resonance ω = ω0, impedance is purely resistive R, so |H(ω0)| = 1.0 (peak).
    // Quality factor: Q = (1 / R) * sqrt(L / C) = ω0 * L / R
    // Half-power (-3 dB) bandwidth: Δω = R / L  =>  Δf = f2 - f1 = R / (2 * π * L) = f0 / Q
    let r: f64 = 100.0;     // 100 Ω
    let l: f64 = 0.01;      // 10 mH = 0.01 H
    let c: f64 = 1.0e-8;    // 10 nF = 1e-8 F

    // Theoretical resonant frequency:
    // f0 = 1 / (2 * π * sqrt(0.01 * 1e-8)) = 1 / (2 * π * 1e-5) ≈ 15915.494 Hz
    let f0_theoretical = 1.0 / (2.0 * PI * (l * c).sqrt());
    // Theoretical Q = (1 / R) * sqrt(L / C) = (1 / 100) * sqrt(0.01 / 1e-8) = 1000 / 100 = 10.0
    let q_theoretical = (1.0 / r) * (l / c).sqrt();

    let deck = format!(
        "Series RLC Band-Pass Filter\n\
         V1 in 0 AC 1\n\
         L1 in n1 {l}\n\
         C1 n1 out {c}\n\
         R1 out 0 {r}\n\
         .ac lin 2401 10000 22000\n\
         .end\n"
    );

    let (dir, _) = simulate("rlc_bp", &deck).await;
    let (headers, rows) = parse_csv_file(&dir.path.join("sim/rlc_bp/rlc_bp-ac.csv"));

    let freq_col = headers.iter().position(|h| h == "freq").unwrap();
    let vm_out_col = headers.iter().position(|h| h == "vm(out)").unwrap();

    // Find the peak magnitude and corresponding frequency:
    let (peak_idx, peak_row) = rows
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a[vm_out_col].total_cmp(&b[vm_out_col]))
        .expect("must have rows");

    let f_peak = peak_row[freq_col];
    let peak_mag = peak_row[vm_out_col];

    let peak_rel_err = (f_peak - f0_theoretical).abs() / f0_theoretical;
    assert!(
        peak_rel_err < 0.005,
        "peak frequency {f_peak} Hz vs theoretical {f0_theoretical} Hz (rel err {peak_rel_err} > 0.5%)"
    );

    // Target half-power level: |H| = peak_mag / sqrt(2) ≈ 0.7071
    let half_power = peak_mag / 2.0f64.sqrt();

    // Find lower -3 dB frequency f1 (where magnitude rises past half_power):
    let idx1 = rows[..peak_idx]
        .iter()
        .rposition(|r| r[vm_out_col] < half_power)
        .expect("must find lower -3dB crossing");
    let (ra0, ra1) = (&rows[idx1], &rows[idx1 + 1]);
    let alpha1 = (half_power - ra0[vm_out_col]) / (ra1[vm_out_col] - ra0[vm_out_col]);
    let f1 = ra0[freq_col] + alpha1 * (ra1[freq_col] - ra0[freq_col]);

    // Find upper -3 dB frequency f2 (where magnitude falls below half_power):
    let idx2 = rows[peak_idx..]
        .iter()
        .position(|r| r[vm_out_col] < half_power)
        .map(|rel| peak_idx + rel)
        .expect("must find upper -3dB crossing");
    let (rb0, rb1) = (&rows[idx2 - 1], &rows[idx2]);
    let alpha2 = (half_power - rb0[vm_out_col]) / (rb1[vm_out_col] - rb0[vm_out_col]);
    let f2 = rb0[freq_col] + alpha2 * (rb1[freq_col] - rb0[freq_col]);

    let measured_bw = f2 - f1;
    let measured_q = f_peak / measured_bw;

    let q_rel_err = (measured_q - q_theoretical).abs() / q_theoretical;
    assert!(
        q_rel_err < 0.02,
        "measured Q {measured_q} (BW={measured_bw} Hz) vs theoretical {q_theoretical} (rel err {q_rel_err} > 2%)"
    );
}

/// 3. `oct` and `lin` sweeps: row counts and spacing per SPICE rules.
#[tokio::test]
async fn test_3_oct_and_lin_frequency_sweeps_and_spacing() {
    // SPICE rules:
    // .ac lin n fstart fstop:
    //   Total rows = n
    //   Spacing is linear: Δf = (fstop - fstart) / (n - 1)
    //   f_i = fstart + i * Δf
    let n_lin = 21;
    let fstart_lin: f64 = 100.0;
    let fstop_lin: f64 = 300.0;
    let deck_lin = format!(
        "Linear Sweep Test\n\
         V1 in 0 AC 1\n\
         R1 in 0 1k\n\
         .ac lin {n_lin} {fstart_lin} {fstop_lin}\n\
         .end\n"
    );
    let (dir_lin, _) = simulate("sweep_lin", &deck_lin).await;
    let (headers_lin, rows_lin) = parse_csv_file(&dir_lin.path.join("sim/sweep_lin/sweep_lin-ac.csv"));
    let freq_col_lin = headers_lin.iter().position(|h| h == "freq").unwrap();

    assert_eq!(rows_lin.len(), n_lin, "lin sweep must have exactly n rows");
    let step_lin = (fstop_lin - fstart_lin) / (n_lin as f64 - 1.0);
    for (i, row) in rows_lin.iter().enumerate() {
        let f = row[freq_col_lin];
        let f_expected = fstart_lin + i as f64 * step_lin;
        assert!(
            (f - f_expected).abs() < 1e-9,
            "lin row {i}: got {f}, expected {f_expected}"
        );
    }

    // .ac oct n fstart fstop:
    //   Total octaves = log2(fstop / fstart)
    //   Points per octave = n
    //   Total rows = octaves * n + 1
    //   Spacing ratio between consecutive points: r = 2^(1 / n)
    //   f_i = fstart * 2^(i / n)
    let n_oct = 4;
    let fstart_oct: f64 = 100.0;
    let fstop_oct: f64 = 800.0; // 800 / 100 = 8 = 2^3 => 3 octaves => 3 * 4 + 1 = 13 rows
    let expected_rows_oct = 3 * n_oct + 1;

    let deck_oct = format!(
        "Octave Sweep Test\n\
         V1 in 0 AC 1\n\
         R1 in 0 1k\n\
         .ac oct {n_oct} {fstart_oct} {fstop_oct}\n\
         .end\n"
    );
    let (dir_oct, _) = simulate("sweep_oct", &deck_oct).await;
    let (headers_oct, rows_oct) = parse_csv_file(&dir_oct.path.join("sim/sweep_oct/sweep_oct-ac.csv"));
    let freq_col_oct = headers_oct.iter().position(|h| h == "freq").unwrap();

    assert_eq!(
        rows_oct.len(),
        expected_rows_oct,
        "oct sweep must have octaves * n + 1 rows"
    );
    for (i, row) in rows_oct.iter().enumerate() {
        let f = row[freq_col_oct];
        let f_expected = fstart_oct * 2.0f64.powf(i as f64 / n_oct as f64);
        let rel_err = (f - f_expected).abs() / f_expected;
        assert!(
            rel_err < 1e-9,
            "oct row {i}: got {f}, expected {f_expected} (rel err {rel_err})"
        );
    }
}

/// 4. AC source phase:
/// `AC 1 90` shifts the output phase by +90° vs `AC 1`; magnitude scales linearly with `AC 2`.
#[tokio::test]
async fn test_4_ac_source_phase_shift_and_magnitude_scaling() {
    // Passive circuit with RC divider so phase is non-zero:
    let r: f64 = 1000.0;
    let c: f64 = 1.0e-7;

    // Base simulation: AC 1 (default phase 0°)
    let deck_base = format!(
        "Base Source AC 1\n\
         V1 in 0 AC 1\n\
         R1 in out {r}\n\
         C1 out 0 {c}\n\
         .ac lin 5 100 1000\n\
         .end\n"
    );
    let (dir_base, _) = simulate("src_base", &deck_base).await;
    let (h_base, rows_base) = parse_csv_file(&dir_base.path.join("sim/src_base/src_base-ac.csv"));
    let vm_base_col = h_base.iter().position(|h| h == "vm(out)").unwrap();
    let vp_base_col = h_base.iter().position(|h| h == "vp(out)").unwrap();

    // Phase shifted simulation: AC 1 90 (amplitude 1, phase +90°)
    let deck_phase = format!(
        "Phase Shifted Source AC 1 90\n\
         V1 in 0 AC 1 90\n\
         R1 in out {r}\n\
         C1 out 0 {c}\n\
         .ac lin 5 100 1000\n\
         .end\n"
    );
    let (dir_phase, _) = simulate("src_phase", &deck_phase).await;
    let (h_phase, rows_phase) = parse_csv_file(&dir_phase.path.join("sim/src_phase/src_phase-ac.csv"));
    let vm_phase_col = h_phase.iter().position(|h| h == "vm(out)").unwrap();
    let vp_phase_col = h_phase.iter().position(|h| h == "vp(out)").unwrap();

    // Amplitude scaled simulation: AC 2 (amplitude 2, default phase 0°)
    let deck_scale = format!(
        "Magnitude Scaled Source AC 2\n\
         V1 in 0 AC 2\n\
         R1 in out {r}\n\
         C1 out 0 {c}\n\
         .ac lin 5 100 1000\n\
         .end\n"
    );
    let (dir_scale, _) = simulate("src_scale", &deck_scale).await;
    let (h_scale, rows_scale) = parse_csv_file(&dir_scale.path.join("sim/src_scale/src_scale-ac.csv"));
    let vm_scale_col = h_scale.iter().position(|h| h == "vm(out)").unwrap();
    let vp_scale_col = h_scale.iter().position(|h| h == "vp(out)").unwrap();

    for i in 0..rows_base.len() {
        let mag_base = rows_base[i][vm_base_col];
        let phase_base = rows_base[i][vp_base_col];

        let mag_p = rows_phase[i][vm_phase_col];
        let phase_p = rows_phase[i][vp_phase_col];

        let mag_s = rows_scale[i][vm_scale_col];
        let phase_s = rows_scale[i][vp_scale_col];

        // Magnitude of AC 1 90 must match AC 1:
        assert!(
            (mag_p - mag_base).abs() / mag_base < 1e-9,
            "row {i}: AC 1 90 magnitude {mag_p} should equal AC 1 magnitude {mag_base}"
        );

        // Phase of AC 1 90 must be shifted by exactly +90° relative to AC 1:
        // phase_p - phase_base = +90°
        let phase_diff = phase_p - phase_base;
        assert!(
            (phase_diff - 90.0).abs() < 1e-6,
            "row {i}: phase diff {phase_diff}° should be exactly +90°"
        );

        // Magnitude of AC 2 must be exactly 2 * magnitude of AC 1 (linear scaling):
        let scale_ratio = mag_s / mag_base;
        assert!(
            (scale_ratio - 2.0).abs() < 1e-9,
            "row {i}: AC 2 / AC 1 ratio {scale_ratio} should be 2.0"
        );

        // Phase of AC 2 must be identical to AC 1:
        assert!(
            (phase_s - phase_base).abs() < 1e-6,
            "row {i}: AC 2 phase {phase_s}° should equal AC 1 phase {phase_base}°"
        );
    }
}

/// 5. Common-source NMOS amplifier (level 1, lambda=0):
/// midband gain = −gm·RD with gm = √(2·KP·W/L·ID) from the `.op` drain current
/// (read -op.txt) within 1 %.
#[tokio::test]
async fn test_5_common_source_nmos_amplifier_midband_gain() {
    // Common-source NMOS amplifier:
    // Supply VDD = 10 V, Gate bias VIN = 2.5 V DC with AC 1.
    // Transistor M1: Drain=out, Gate=in, Source=0, Bulk=0.
    // Drain resistor RD = 5000 Ω connected from vdd to out.
    // Parameters: Level 1 NMOS, VTO = 1.0 V, KP = 1e-4 A/V^2, LAMBDA = 0.
    // W = 100 µm, L = 100 µm => W/L = 1.0.
    // Transistor operates in saturation (Vds > Vgs - Vth = 1.5 V).
    // Closed-form transconductance:
    // gm = d(ID)/d(Vgs) = KP * (W/L) * (Vgs - Vth) = sqrt(2 * KP * (W/L) * ID)
    // Midband small-signal voltage gain:
    // Av = v(out) / v(in) = -gm * RD
    // Magnitude: |Av| = gm * RD
    // Phase: ∠Av = 180°
    let vdd_val = 10.0;
    let vin_dc = 2.5;
    let rd = 5000.0;
    let kp = 1.0e-4;
    let w = 100.0e-6;
    let l = 100.0e-6;

    let deck = format!(
        "Common-Source NMOS Amplifier\n\
         VDD vdd 0 DC {vdd_val}\n\
         VIN in 0 DC {vin_dc} AC 1\n\
         RD vdd out {rd}\n\
         M1 out in 0 0 nmos w=100u l=100u\n\
         .model nmos NMOS(level=1 vto=1.0 kp={kp} lambda=0)\n\
         .op\n\
         .ac lin 1 1000 1000\n\
         .end\n"
    );

    let (dir, _) = simulate("cs_nmos", &deck).await;

    // 1. Read drain current from -op.txt:
    let op_map = parse_op_file(&dir.path.join("sim/cs_nmos/cs_nmos-op.txt"));
    // Current through RD: ID = (V(vdd) - V(out)) / RD, or directly from branch current |i(vdd)|
    let id_from_v = (op_map["v(vdd)"] - op_map["v(out)"]) / rd;
    let id = if let Some(i_vdd) = op_map.get("i(vdd)") {
        i_vdd.abs()
    } else {
        id_from_v
    };

    assert!(id > 0.0, "drain current ID must be positive");
    assert!(
        (id - id_from_v).abs() / id < 1e-6,
        "branch current and resistor ohm's law must match"
    );

    // Theoretical gm derived from closed-form formula in brief: gm = sqrt(2 * KP * (W/L) * ID)
    let gm = (2.0 * kp * (w / l) * id).sqrt();
    let expected_midband_gain = gm * rd; // magnitude |-gm * RD|

    // 2. Read AC simulation output from -ac.csv:
    let (headers, rows) = parse_csv_file(&dir.path.join("sim/cs_nmos/cs_nmos-ac.csv"));
    let vm_out_col = headers.iter().position(|h| h == "vm(out)").unwrap();
    let vp_out_col = headers.iter().position(|h| h == "vp(out)").unwrap();

    let sim_gain_mag = rows[0][vm_out_col];
    let sim_gain_phase = rows[0][vp_out_col];

    // Check magnitude within 1 %:
    let gain_rel_err = (sim_gain_mag - expected_midband_gain).abs() / expected_midband_gain;
    assert!(
        gain_rel_err < 0.01,
        "sim gain {sim_gain_mag} vs theoretical gm*RD {expected_midband_gain} (rel err {gain_rel_err} > 1%)"
    );

    // Check inverted phase (180° for negative gain):
    let phase_err = (sim_gain_phase - 180.0).abs();
    assert!(
        phase_err < 0.5,
        "phase {sim_gain_phase}° should be 180° (inverting amplifier)"
    );
}

/// 6. Diode small-signal: forward-biased diode via resistor,
/// AC gain to the diode node = rd/(R+rd) with rd = N·Vt/ID within 1 %.
#[tokio::test]
async fn test_6_diode_small_signal_resistance_and_gain() {
    // Diode small-signal model:
    // DC bias: V1 (2 V) drives series resistor R1 (1000 Ω) into diode D1 to ground.
    // Diode forward-biased with current ID = (V1 - VD) / R1.
    // Small-signal dynamic resistance: rd = d(VD)/d(ID) = N * Vt / (ID + IS) ≈ N * Vt / ID.
    // Thermal voltage: Vt = k * T / q at 27 °C.
    // AC divider gain from source to diode anode (node out):
    // Av = rd / (R + rd)
    let v_dc = 2.0;
    let r = 1000.0;
    let n_factor = 1.0;
    let is_sat = 1.0e-14;

    let deck = format!(
        "Diode Small-Signal Analysis\n\
         V1 in 0 DC {v_dc} AC 1\n\
         R1 in out {r}\n\
         D1 out 0 d\n\
         .model d D(is={is_sat} n={n_factor})\n\
         .op\n\
         .ac lin 1 1000 1000\n\
         .end\n"
    );

    let (dir, _) = simulate("diode_ss", &deck).await;

    // 1. Read DC operating point:
    let op_map = parse_op_file(&dir.path.join("sim/diode_ss/diode_ss-op.txt"));
    let v_in = op_map["v(in)"];
    let v_out = op_map["v(out)"];
    let id = (v_in - v_out) / r;

    assert!(id > 0.0, "diode must be forward biased with positive ID");

    // Theoretical dynamic resistance: rd = N * Vt / ID
    let vt = thermal_voltage(27.0);
    let rd = n_factor * vt / id;

    // Closed-form small-signal AC gain: Av = rd / (R + rd)
    let expected_gain = rd / (r + rd);

    // 2. Read AC simulation output:
    let (headers, rows) = parse_csv_file(&dir.path.join("sim/diode_ss/diode_ss-ac.csv"));
    let vm_out_col = headers.iter().position(|h| h == "vm(out)").unwrap();

    let sim_gain = rows[0][vm_out_col];
    let gain_rel_err = (sim_gain - expected_gain).abs() / expected_gain;
    assert!(
        gain_rel_err < 0.01,
        "sim diode gain {sim_gain} vs theoretical {expected_gain} (rel err {gain_rel_err} > 1%)"
    );
}

/// 7. `.noise` of a single resistor R to ground driven through a source with series R:
/// output noise density = √(4kT·R_parallel) within 0.1 %; at `.temp 85` it scales with √T.
/// Integrated total = density·√(fstop−fstart) for a flat spectrum within 0.5 %.
#[tokio::test]
async fn test_7_resistor_noise_density_temperature_and_integral() {
    // Noise circuit:
    // Source V1 with series resistor R1 (1000 Ω) driving node out.
    // Resistor R2 (1000 Ω) to ground from node out.
    // Small-signal Thévenin/Norton equivalent resistance at node out to AC ground:
    // R_parallel = R1 || R2 = 1000 * 1000 / (1000 + 1000) = 500 Ω.
    // Resistor thermal noise current generators: in1^2 = 4kT/R1, in2^2 = 4kT/R2.
    // Both noise currents inject into the parallel resistance R_parallel:
    // v_out_noise^2 = (in1^2 + in2^2) * R_parallel^2
    //               = 4kT * (1/R1 + 1/R2) * R_parallel^2
    //               = 4kT * R_parallel (in V^2 / Hz).
    // Output noise spectral density: onoise = sqrt(4 * k * T * R_parallel) (in V / sqrt(Hz)).
    let r1: f64 = 1000.0;
    let r2: f64 = 1000.0;
    let r_parallel: f64 = (r1 * r2) / (r1 + r2); // 500.0 Ω

    let fstart: f64 = 100.0;
    let fstop: f64 = 10000.0;

    // Simulation at default temperature 27 °C:
    let deck_27 = format!(
        "Resistor Noise Analysis at 27C\n\
         V1 in 0 AC 1\n\
         R1 in out {r1}\n\
         R2 out 0 {r2}\n\
         .noise v(out) V1 lin 10 {fstart} {fstop}\n\
         .end\n"
    );
    let (dir_27, res_27) = simulate("r_noise_27", &deck_27).await;
    let (h_27, rows_27) = parse_csv_file(&dir_27.path.join("sim/r_noise_27/r_noise_27-noise.csv"));
    let onoise_col_27 = h_27.iter().position(|h| h == "onoise_spectrum").unwrap();

    let t_kelvin_27 = to_kelvin(27.0);
    // Closed-form noise density: sqrt(4 * k * T * R_parallel)
    let density_27_theoretical = (4.0 * BOLTZMANN_K * t_kelvin_27 * r_parallel).sqrt();

    for (i, row) in rows_27.iter().enumerate() {
        let density_sim = row[onoise_col_27];
        let rel_err = (density_sim - density_27_theoretical).abs() / density_27_theoretical;
        assert!(
            rel_err < 0.001,
            "row {i}: onoise {density_sim} vs theoretical {density_27_theoretical} (rel err {rel_err} > 0.1%)"
        );
    }

    // Integrated total for a flat spectrum:
    // V_total = density * sqrt(fstop - fstart)
    let total_27_theoretical = density_27_theoretical * (fstop - fstart).sqrt();

    // Parse the total integrated output noise reported in tool text:
    let marker = "total integrated output noise ";
    let total_sim_27: f64 = res_27
        .text
        .split(marker)
        .nth(1)
        .expect("text should contain total integrated output noise")
        .split_whitespace()
        .next()
        .expect("number after marker")
        .parse()
        .expect("valid f64");

    let total_rel_err = (total_sim_27 - total_27_theoretical).abs() / total_27_theoretical;
    assert!(
        total_rel_err < 0.005,
        "integrated noise {total_sim_27} vs theoretical {total_27_theoretical} (rel err {total_rel_err} > 0.5%)"
    );

    // Simulation at temperature 85 °C:
    let deck_85 = format!(
        "Resistor Noise Analysis at 85C\n\
         V1 in 0 AC 1\n\
         R1 in out {r1}\n\
         R2 out 0 {r2}\n\
         .noise v(out) V1 lin 10 {fstart} {fstop}\n\
         .temp 85\n\
         .end\n"
    );
    let (dir_85, _) = simulate("r_noise_85", &deck_85).await;
    let (h_85, rows_85) = parse_csv_file(&dir_85.path.join("sim/r_noise_85/r_noise_85-noise-85C.csv"));
    let onoise_col_85 = h_85.iter().position(|h| h == "onoise_spectrum").unwrap();

    let t_kelvin_85 = to_kelvin(85.0);
    let density_85_theoretical = (4.0 * BOLTZMANN_K * t_kelvin_85 * r_parallel).sqrt();

    for (i, row) in rows_85.iter().enumerate() {
        let density_sim_85 = row[onoise_col_85];
        let rel_err = (density_sim_85 - density_85_theoretical).abs() / density_85_theoretical;
        assert!(
            rel_err < 0.001,
            "row {i} at 85C: onoise {density_sim_85} vs theoretical {density_85_theoretical} (rel err {rel_err} > 0.1%)"
        );
    }

    // Check that density scales precisely with sqrt(T):
    let ratio_sim = rows_85[0][onoise_col_85] / rows_27[0][onoise_col_27];
    let ratio_theoretical = (t_kelvin_85 / t_kelvin_27).sqrt();
    let ratio_err = (ratio_sim - ratio_theoretical).abs() / ratio_theoretical;
    assert!(
        ratio_err < 0.001,
        "temperature scale ratio {ratio_sim} vs theoretical {ratio_theoretical} (rel err {ratio_err} > 0.1%)"
    );
}

/// 8. `.temp -40 27 85` on a diode at constant current:
/// Vd decreases with temperature (≈ −2 mV/°C, within ±1 mV/°C); files carry `-{t}C` suffixes;
/// a single `.temp 27` (or none) keeps the old file names.
/// Resistor `tc1=0.004`: R(T) = R0(1+tc1·(T−27)).
#[tokio::test]
async fn test_8_temperature_sweep_diode_and_resistor_tc1() {
    // 1. Diode temperature sweep:
    // Constant current 1 mA into diode.
    let deck_diode = "\
        Diode Temperature Sweep\n\
        I1 0 out 1m\n\
        D1 out 0 d\n\
        .model d D(is=1e-14 n=1.0)\n\
        .temp -40 27 85\n\
        .op\n\
        .end\n";

    let (dir_diode, res_diode) = simulate("diode_temp", deck_diode).await;

    // Files must have -{t}C suffix:
    let expected_diode_files = [
        "sim/diode_temp/diode_temp-op--40C.txt",
        "sim/diode_temp/diode_temp-op-27C.txt",
        "sim/diode_temp/diode_temp-op-85C.txt",
    ];
    for file in &expected_diode_files {
        assert!(
            res_diode.files.contains(&file.to_string()),
            "missing expected diode temp file: {file}"
        );
    }

    let op_neg40 = parse_op_file(&dir_diode.path.join(expected_diode_files[0]));
    let op_27 = parse_op_file(&dir_diode.path.join(expected_diode_files[1]));
    let op_85 = parse_op_file(&dir_diode.path.join(expected_diode_files[2]));

    let vd_neg40 = op_neg40["v(out)"];
    let vd_27 = op_27["v(out)"];
    let vd_85 = op_85["v(out)"];

    // Vd must strictly decrease with increasing temperature:
    assert!(
        vd_neg40 > vd_27 && vd_27 > vd_85,
        "Vd must decrease with T: Vd(-40)={vd_neg40}, Vd(27)={vd_27}, Vd(85)={vd_85}"
    );

    // Temperature coefficient dVd/dT ≈ -2 mV/°C within ±1 mV/°C (i.e. -3 mV/°C to -1 mV/°C):
    let slope_neg40_to_27 = (vd_27 - vd_neg40) / (27.0 - (-40.0));
    let slope_27_to_85 = (vd_85 - vd_27) / (85.0 - 27.0);

    assert!(
        slope_neg40_to_27 >= -0.003 && slope_neg40_to_27 <= -0.001,
        "slope from -40 to 27 °C is {slope_neg40_to_27} V/°C, expected around -2 mV/°C ± 1 mV/°C"
    );
    assert!(
        slope_27_to_85 >= -0.003 && slope_27_to_85 <= -0.001,
        "slope from 27 to 85 °C is {slope_27_to_85} V/°C, expected around -2 mV/°C ± 1 mV/°C"
    );

    // 2. Single .temp 27 (or none) keeps old file names (no temperature suffix):
    let deck_no_temp = "\
        No Temp Directive\n\
        V1 in 0 5\n\
        R1 in 0 1k\n\
        .op\n\
        .end\n";
    let (_, res_no_temp) = simulate("no_temp", deck_no_temp).await;
    assert_eq!(
        res_no_temp.files,
        vec!["sim/no_temp/no_temp-op.txt".to_string()],
        "no .temp directive must keep standard filename without suffix"
    );

    let deck_temp_27 = "\
        Single Temp 27\n\
        V1 in 0 5\n\
        R1 in 0 1k\n\
        .temp 27\n\
        .op\n\
        .end\n";
    let (_, res_temp_27) = simulate("temp_27", deck_temp_27).await;
    assert_eq!(
        res_temp_27.files,
        vec!["sim/temp_27/temp_27-op.txt".to_string()],
        "single .temp 27 must keep standard filename without suffix"
    );

    // 3. Resistor tc1=0.004: R(T) = R0 * (1 + tc1 * (T - 27)):
    let r0: f64 = 1000.0;
    let tc1: f64 = 0.004;
    let deck_res_tc = format!(
        "Resistor TC1 Sweep\n\
         V1 in 0 1.0\n\
         R1 in 0 {r0} tc1={tc1}\n\
         .temp -40 27 85\n\
         .op\n\
         .end\n"
    );
    let (dir_res_tc, res_res_tc) = simulate("r_tc", &deck_res_tc).await;

    let expected_r_files = [
        (-40.0, "sim/r_tc/r_tc-op--40C.txt"),
        (27.0, "sim/r_tc/r_tc-op-27C.txt"),
        (85.0, "sim/r_tc/r_tc-op-85C.txt"),
    ];

    for (temp_c, file_name) in expected_r_files {
        assert!(res_res_tc.files.contains(&file_name.to_string()));
        let op = parse_op_file(&dir_res_tc.path.join(file_name));
        let i_v1 = op["i(v1)"].abs();
        let r_meas = 1.0 / i_v1;

        // Closed-form formula: R(T) = R0 * (1 + tc1 * (T - 27))
        let r_expected = r0 * (1.0 + tc1 * (temp_c - 27.0));
        let r_rel_err = (r_meas - r_expected).abs() / r_expected;
        assert!(
            r_rel_err < 1e-6,
            "at {temp_c} °C: measured R {r_meas} vs theoretical {r_expected} (rel err {r_rel_err} > 1e-6)"
        );
    }
}

/// 9. Every new error message in the brief, one assertion each, with correct line numbers.
#[tokio::test]
async fn test_9_new_error_messages_with_line_numbers() {
    // 1. line {n}: .ac needs dec, oct or lin
    let deck_ac_bad_mode = "\
        Bad AC Mode\n\
        V1 in 0 AC 1\n\
        R1 in 0 1k\n\
        .ac bad 1 1 10\n\
        .end\n";
    let (_, err_ac_mode) = simulate_err("err_ac_mode", deck_ac_bad_mode).await;
    assert_eq!(err_ac_mode, "line 4: .ac needs dec, oct or lin");

    // 2. line {n}: .ac needs fstart > 0 and fstop >= fstart (case A: fstart <= 0)
    let deck_ac_zero_start = "\
        Zero Start Frequency\n\
        V1 in 0 AC 1\n\
        R1 in 0 1k\n\
        .ac dec 1 0 10\n\
        .end\n";
    let (_, err_ac_zero) = simulate_err("err_ac_zero", deck_ac_zero_start).await;
    assert_eq!(err_ac_zero, "line 4: .ac needs fstart > 0 and fstop >= fstart");

    // 3. line {n}: .ac needs fstart > 0 and fstop >= fstart (case B: fstop < fstart)
    let deck_ac_descending = "\
        Descending Frequencies\n\
        V1 in 0 AC 1\n\
        R1 in 0 1k\n\
        .ac lin 1 10 1\n\
        .end\n";
    let (_, err_ac_desc) = simulate_err("err_ac_desc", deck_ac_descending).await;
    assert_eq!(err_ac_desc, "line 4: .ac needs fstart > 0 and fstop >= fstart");

    // 4. line {n}: .noise needs an output node and an independent source (case A: unknown source)
    let deck_noise_absent_src = "\
        Noise Unknown Source\n\
        V1 out 0 AC 1\n\
        R1 out 0 1k\n\
        .noise v(out) absent dec 1 1 10\n\
        .end\n";
    let (_, err_noise_src) = simulate_err("err_noise_src", deck_noise_absent_src).await;
    assert_eq!(err_noise_src, "line 4: .noise needs an output node and an independent source");

    // 5. line {n}: .noise needs an output node and an independent source (case B: unknown output node)
    let deck_noise_absent_node = "\
        Noise Unknown Output Node\n\
        V1 out 0 AC 1\n\
        R1 out 0 1k\n\
        .noise v(absent) V1 dec 1 1 10\n\
        .end\n";
    let (_, err_noise_node) = simulate_err("err_noise_node", deck_noise_absent_node).await;
    assert_eq!(err_noise_node, "line 4: .noise needs an output node and an independent source");

    // 6. line {n}: .noise needs an output node and an independent source (case C: non-voltage output)
    let deck_noise_invalid_out = "\
        Noise Invalid Output Specification\n\
        V1 out 0 AC 1\n\
        R1 out 0 1k\n\
        .noise i(out) V1 dec 1 1 10\n\
        .end\n";
    let (_, err_noise_i) = simulate_err("err_noise_i", deck_noise_invalid_out).await;
    assert_eq!(err_noise_i, "line 4: .noise needs an output node and an independent source");
}

/// 10. Old behaviour unchanged:
/// a deck with only `.op`/`.tran` produces the same files as before (no `-ac`/`-noise` files, no suffix).
#[tokio::test]
async fn test_10_old_behaviour_unchanged_op_and_tran() {
    let deck = "\
        Legacy Op and Tran Deck\n\
        V1 in 0 5\n\
        R1 in out 1k\n\
        C1 out 0 1u\n\
        .op\n\
        .tran 10u 100u\n\
        .end\n";

    let (dir, res) = simulate("legacy", deck).await;

    // Must produce exactly the 3 legacy files:
    let expected_files = [
        "sim/legacy/legacy-op.txt",
        "sim/legacy/legacy-tran.csv",
        "sim/legacy/legacy-tran.svg",
    ];
    assert_eq!(
        res.files, expected_files,
        "legacy deck files must match exactly"
    );

    // Verify no -ac, no -noise, and no temperature suffix in any file:
    for file in &res.files {
        assert!(
            !file.contains("-ac"),
            "legacy run must not produce -ac files: {file}"
        );
        assert!(
            !file.contains("-noise"),
            "legacy run must not produce -noise files: {file}"
        );
        assert!(
            !file.contains("C."),
            "legacy run must not have temperature suffix: {file}"
        );
        // Ensure file exists on disk and is non-empty:
        let meta = fs::metadata(dir.path.join(file)).expect("file must exist on disk");
        assert!(meta.len() > 0, "file {file} must not be empty");
    }
}

/// 11. Determinism:
/// two runs → byte-identical files; Bode SVG starts with `<svg` and has two panels/axes labels for dB and phase.
#[tokio::test]
async fn test_11_determinism_and_bode_svg_structure() {
    let deck = "\
        Determinism and Bode SVG Deck\n\
        V1 in 0 AC 1\n\
        R1 in out 1k\n\
        C1 out 0 100n\n\
        .ac dec 10 1 100k\n\
        .noise v(out) V1 lin 5 100 10k\n\
        .end\n";

    // Run 1:
    let (dir1, res1) = simulate("det_run1", deck).await;
    // Run 2:
    let (dir2, res2) = simulate("det_run2", deck).await;

    assert_eq!(res1.files.len(), 4, "expected 4 files (ac csv/svg, noise csv/svg)");
    assert_eq!(res1.files.len(), res2.files.len());

    // Compare byte-for-byte content of all output files between the two runs:
    for (f1, f2) in res1.files.iter().zip(&res2.files) {
        let bytes1 = fs::read(dir1.path.join(f1)).expect("read file from run 1");
        let bytes2 = fs::read(dir2.path.join(f2)).expect("read file from run 2");
        assert_eq!(
            bytes1, bytes2,
            "files {f1} and {f2} must be byte-identical between runs"
        );
    }

    // Bode SVG structure inspection:
    let ac_svg_content = fs::read_to_string(dir1.path.join("sim/det_run1/det_run1-ac.svg"))
        .expect("read bode svg");

    // Must start with `<svg`:
    assert!(
        ac_svg_content.starts_with("<svg"),
        "Bode plot SVG must start with '<svg'"
    );

    // Must contain two panels:
    // Output generator embeds a second inner svg with y=\"460\":
    assert!(
        ac_svg_content.contains("<svg y=\"460\""),
        "Bode plot SVG must contain two panels (second panel at y=460)"
    );

    // Must have axes labels for dB and phase:
    assert!(
        ac_svg_content.contains("magnitude (dB)"),
        "Bode plot SVG must contain axis label 'magnitude (dB)'"
    );
    assert!(
        ac_svg_content.contains("phase (degrees)"),
        "Bode plot SVG must contain axis label 'phase (degrees)'"
    );
}
