mod common;
#[path = "common/tools.rs"]
mod support;

use common::TempDir;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

async fn simulate(name: &str, deck: &str) -> (TempDir, forge_server::tools::ToolResult) {
    let rel_path = format!("sim/{name}.cir");
    let dir = support::project("bb_sim", &[(&rel_path, deck)]);
    let res = support::run("circuit_simulate", json!({"source": rel_path}), &dir.path)
        .await
        .expect("circuit_simulate should succeed");
    (dir, res)
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

fn interpolate_csv(headers: &[String], rows: &[Vec<f64>], col_name: &str, t: f64) -> f64 {
    let col = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case(col_name))
        .unwrap_or_else(|| panic!("column {col_name} not found in headers: {headers:?}"));
    let upper = rows.partition_point(|r| r[0] < t);
    if upper == 0 {
        return rows[0][col];
    }
    if upper >= rows.len() {
        return rows.last().unwrap()[col];
    }
    let (r0, r1) = (&rows[upper - 1], &rows[upper]);
    let t0 = r0[0];
    let t1 = r1[0];
    if (t1 - t0).abs() < 1e-15 {
        return r0[col];
    }
    r0[col] + (r1[col] - r0[col]) * (t - t0) / (t1 - t0)
}

/// 1. KCL check: a 5-resistor bridge with a current source; recompute the node voltages
/// by solving the 3x3 nodal system in the test; match to 1e-9 relative.
#[tokio::test]
async fn test_1_kcl_five_resistor_bridge_with_current_source() {
    // 5-resistor bridge circuit with 3 non-ground nodes:
    // Node 0: Ground (GND)
    // Node 1: Input node where current source injects I_in
    // Node 2, 3: Bridge divider nodes
    // Resistors:
    // R1 between 1 and 2
    // R2 between 1 and 3
    // R3 between 2 and 3 (the cross-bridge resistor)
    // R4 between 2 and 0
    // R5 between 3 and 0
    let r1: f64 = 100.0;
    let r2: f64 = 200.0;
    let r3: f64 = 50.0;
    let r4: f64 = 300.0;
    let r5: f64 = 400.0;
    let i_in: f64 = 0.05; // 50 mA

    let deck = format!(
        "5-Resistor Bridge KCL\n\
         I1 0 1 {i_in}\n\
         R1 1 2 {r1}\n\
         R2 1 3 {r2}\n\
         R3 2 3 {r3}\n\
         R4 2 0 {r4}\n\
         R5 3 0 {r5}\n\
         .op\n\
         .end\n"
    );

    let (dir, _) = simulate("bridge", &deck).await;
    let op_map = parse_op_file(&dir.path.join("sim/bridge/bridge-op.txt"));

    // Derive node voltages by setting up the 3x3 nodal conductance system A * V = b:
    // Conductances G_k = 1 / R_k
    let g1 = 1.0 / r1;
    let g2 = 1.0 / r2;
    let g3 = 1.0 / r3;
    let g4 = 1.0 / r4;
    let g5 = 1.0 / r5;

    // Kirchhoff's Current Law (sum of currents leaving each node = sum of injected currents):
    // Node 1: (V1 - V2)*G1 + (V1 - V3)*G2 = I_in
    //         => (G1 + G2)*V1 - G1*V2 - G2*V3 = I_in
    // Node 2: (V2 - V1)*G1 + (V2 - V3)*G3 + V2*G4 = 0
    //         => -G1*V1 + (G1 + G3 + G4)*V2 - G3*V3 = 0
    // Node 3: (V3 - V1)*G2 + (V3 - V2)*G3 + V3*G5 = 0
    //         => -G2*V1 - G3*V2 + (G2 + G3 + G5)*V3 = 0
    let a: [[f64; 3]; 3] = [
        [g1 + g2, -g1, -g2],
        [-g1, g1 + g3 + g4, -g3],
        [-g2, -g3, g2 + g3 + g5],
    ];
    let b: [f64; 3] = [i_in, 0.0, 0.0];

    // Solve 3x3 system via Cramer's rule:
    let det = |m: [[f64; 3]; 3]| -> f64 {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };

    let det_a = det(a);
    assert!(det_a.abs() > 1e-12, "singular nodal matrix");

    let mut a1 = a;
    for r in 0..3 { a1[r][0] = b[r]; }
    let mut a2 = a;
    for r in 0..3 { a2[r][1] = b[r]; }
    let mut a3 = a;
    for r in 0..3 { a3[r][2] = b[r]; }

    let v1_ref = det(a1) / det_a;
    let v2_ref = det(a2) / det_a;
    let v3_ref = det(a3) / det_a;

    let v1_sim = op_map["v(1)"];
    let v2_sim = op_map["v(2)"];
    let v3_sim = op_map["v(3)"];

    let rel_err_1 = (v1_sim - v1_ref).abs() / v1_ref;
    let rel_err_2 = (v2_sim - v2_ref).abs() / v2_ref;
    let rel_err_3 = (v3_sim - v3_ref).abs() / v3_ref;

    assert!(rel_err_1 < 1e-9, "V1 mismatch: sim={v1_sim}, ref={v1_ref}, rel_err={rel_err_1}");
    assert!(rel_err_2 < 1e-9, "V2 mismatch: sim={v2_sim}, ref={v2_ref}, rel_err={rel_err_2}");
    assert!(rel_err_3 < 1e-9, "V3 mismatch: sim={v3_sim}, ref={v3_ref}, rel_err={rel_err_3}");
}

/// 2. RC low-pass driven by `PULSE(0 1 0 1n 1n 5m 10m)`, R=1k C=1u: at t = 1, 2, 3 ms
/// the output matches 1-e^{-t/RC} within 0.5 %; the CSV time column is strictly increasing
/// and ends at tstop.
#[tokio::test]
async fn test_2_rc_low_pass_step_response() {
    let r: f64 = 1000.0;
    let c: f64 = 1e-6;
    let tau: f64 = r * c; // 1.0 ms
    let tstop: f64 = 5e-3;

    let deck = format!(
        "RC low pass pulse\n\
         V1 in 0 PULSE(0 1 0 1n 1n 5m 10m)\n\
         R1 in out {r}\n\
         C1 out 0 {c}\n\
         .tran 10u {tstop}\n\
         .end\n"
    );

    let (dir, _) = simulate("rc_pulse", &deck).await;
    let (headers, rows) = parse_csv_file(&dir.path.join("sim/rc_pulse/rc_pulse-tran.csv"));

    assert!(!rows.is_empty(), "tran CSV must have rows");

    // CSV time column is strictly increasing
    assert!(
        rows.windows(2).all(|w| w[1][0] > w[0][0]),
        "CSV time column must be strictly increasing"
    );

    // CSV time column ends at tstop
    let last_time = rows.last().unwrap()[0];
    assert!(
        (last_time - tstop).abs() < 1e-9,
        "CSV time column must end at tstop ({tstop}), got {last_time}"
    );

    // At t = 1 ms, 2 ms, 3 ms, verify V_out matches 1 - exp(-t / RC) within 0.5 %
    for t_ms in [1.0f64, 2.0f64, 3.0f64] {
        let t: f64 = t_ms * 1e-3;
        // Closed-form step response of 1st-order RC filter: V(t) = V_final * (1 - exp(-t / tau))
        let v_ref: f64 = 1.0 - (-t / tau).exp();
        let v_sim = interpolate_csv(&headers, &rows, "v(out)", t);
        let rel_err = (v_sim - v_ref).abs() / v_ref;
        assert!(
            rel_err < 0.005,
            "at t = {t_ms} ms: sim={v_sim}, ref={v_ref}, rel_err={rel_err} (limit 0.5 %)"
        );
    }
}

/// 3. LC tank with `ic=` on the capacitor and `uic`: oscillation period from zero crossings
/// = 2π√(LC) within 1 %.
#[tokio::test]
async fn test_3_lc_tank_oscillation_period() {
    let l_val: f64 = 1e-3; // 1 mH
    let c_val: f64 = 1e-6; // 1 uF
    let v_ic: f64 = 1.0;

    // Natural undamped resonant frequency and period:
    // omega_0 = 1 / sqrt(L * C)
    // Period T = 2 * pi * sqrt(L * C)
    let t_theory: f64 = 2.0 * std::f64::consts::PI * (l_val * c_val).sqrt();

    let deck = format!(
        "LC Tank Oscillation\n\
         C1 1 0 {c_val} ic={v_ic}\n\
         L1 1 0 {l_val}\n\
         .tran 2u 2m uic\n\
         .end\n"
    );

    let (dir, _) = simulate("lc_tank", &deck).await;
    let (headers, rows) = parse_csv_file(&dir.path.join("sim/lc_tank/lc_tank-tran.csv"));

    let v1_col = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("v(1)"))
        .expect("v(1) column in csv");

    // Extract zero crossings (falling transitions: v_k >= 0 and v_{k+1} < 0)
    let mut falling_crossings = Vec::new();
    for window in rows.windows(2) {
        let (r0, r1) = (&window[0], &window[1]);
        let (t0, v0) = (r0[0], r0[v1_col]);
        let (t1, v1) = (r1[0], r1[v1_col]);
        if v0 >= 0.0 && v1 < 0.0 {
            // Linear interpolation to find the exact zero crossing time
            let t_zero = t0 + (0.0 - v0) * (t1 - t0) / (v1 - v0);
            falling_crossings.push(t_zero);
        }
    }

    assert!(
        falling_crossings.len() >= 5,
        "expected multiple oscillation cycles, got {}",
        falling_crossings.len()
    );

    // Period from consecutive falling zero crossings
    let first = *falling_crossings.first().unwrap();
    let last = *falling_crossings.last().unwrap();
    let cycles = (falling_crossings.len() - 1) as f64;
    let t_measured = (last - first) / cycles;

    let rel_err = (t_measured - t_theory).abs() / t_theory;
    assert!(
        rel_err < 0.01,
        "LC period mismatch: measured={t_measured:.6e}, theory={t_theory:.6e}, rel_err={rel_err:.6e} (limit 1 %)"
    );
}

/// 4. Diode forward voltage at 1 mA (IS=1e-14, N=1, 300.15 K thermal voltage as SPICE uses):
/// Vd = N·Vt·ln(1+I/IS) within 1 mV (use a current source into the diode).
#[tokio::test]
async fn test_4_diode_forward_voltage() {
    // Thermal voltage Vt at SPICE nominal temperature T = 300.15 K (27 C):
    // k = 1.380649e-23 J/K, q = 1.602176634e-19 C
    // k / q = 8.617333262145e-5 V/K
    // Vt = (k/q) * 300.15 K = 0.025864925786... V
    let vt: f64 = 8.617333262145e-5 * 300.15;
    let is_param: f64 = 1e-14;
    let n: f64 = 1.0;
    let i_forward: f64 = 1e-3; // 1 mA

    // Closed-form Shockley diode equation: I = IS * (exp(Vd / (N * Vt)) - 1)
    // Inverting for forward voltage: Vd = N * Vt * ln(1 + I / IS)
    let vd_theory: f64 = n * vt * (1.0 + i_forward / is_param).ln();

    let deck = format!(
        "Diode Forward Voltage\n\
         I1 0 1 {i_forward}\n\
         D1 1 0 DMOD\n\
         .model DMOD D (IS={is_param} N={n})\n\
         .op\n\
         .end\n"
    );

    let (dir, _) = simulate("diode_fwd", &deck).await;
    let op_map = parse_op_file(&dir.path.join("sim/diode_fwd/diode_fwd-op.txt"));

    let vd_sim = op_map["v(1)"];
    let diff = (vd_sim - vd_theory).abs();
    assert!(
        diff < 0.001,
        "Diode forward voltage: sim={vd_sim:.6e}, theory={vd_theory:.6e}, diff={diff:.6e} (limit 1 mV)"
    );
}

/// 5. Zener-style breakdown: diode with BV=5.1 IBV=1m reverse biased through 1k from 10 V:
/// |Vd| within 50 mV of 5.1.
#[tokio::test]
async fn test_5_zener_diode_breakdown() {
    let bv: f64 = 5.1;
    let ibv: f64 = 1e-3;
    let r_bias: f64 = 1000.0;
    let v_bias: f64 = 10.0;

    // Diode reverse biased:
    // Voltage source of 10V pulls node 1 up through 1k resistor.
    // Diode connected with anode at 0 (GND) and cathode at node 1.
    // Hence junction voltage Vd = Vanode - Vcathode = 0 - V(1) = -V(1).
    // In breakdown, V(1) sits close to BV = 5.1 V.
    let deck = format!(
        "Zener Breakdown\n\
         V1 in 0 {v_bias}\n\
         R1 in 1 {r_bias}\n\
         D1 0 1 DZ\n\
         .model DZ D (BV={bv} IBV={ibv})\n\
         .op\n\
         .end\n"
    );

    let (dir, _) = simulate("zener", &deck).await;
    let op_map = parse_op_file(&dir.path.join("sim/zener/zener-op.txt"));

    let v_node1 = op_map["v(1)"];
    let abs_vd = v_node1; // |Vd| = |0 - V(1)| = V(1)
    let diff = (abs_vd - bv).abs();
    assert!(
        diff < 0.050,
        "Zener breakdown voltage |Vd|={abs_vd:.6e}, BV={bv}, diff={diff:.6e} (limit 50 mV)"
    );
}

/// 6. NMOS in saturation and PMOS mirror: level 1 square-law Id within 1e-4 relative
/// (include lambda); body effect raises Vt for Vsb > 0 per Vt = VTO + γ(√(2φ+Vsb) − √(2φ)).
#[tokio::test]
async fn test_6_nmos_saturation_body_effect_and_pmos_mirror() {
    let vto_n: f64 = 1.0;
    let kp_n: f64 = 200e-6;
    let lambda_n: f64 = 0.02;
    let gamma_n: f64 = 0.5;
    let phi_n: f64 = 0.6; // In SPICE level 1 parameter definition, PHI represents 2*phi_F
    let w_n: f64 = 10e-6;
    let l_n: f64 = 1e-6;

    // Part A: NMOS in saturation with Vsb = 0
    let vds: f64 = 5.0;
    let vgs: f64 = 3.0;
    // With Vsb = 0: Vt = VTO = 1.0 V
    // Overdrive Vov = Vgs - Vt = 3.0 - 1.0 = 2.0 V
    // Saturation condition: Vds (5V) >= Vov (2V), which holds.
    // Shichman-Hodges Level 1 square-law in saturation:
    // Id = (KP / 2) * (W / L) * (Vgs - Vt)^2 * (1 + lambda * Vds)
    let id_sat_theory: f64 = (kp_n / 2.0) * (w_n / l_n) * (vgs - vto_n).powi(2) * (1.0 + lambda_n * vds);

    let deck_nmos = format!(
        "NMOS Saturation\n\
         Vd d 0 {vds}\n\
         Vg g 0 {vgs}\n\
         Vs s 0 0\n\
         Vb b 0 0\n\
         M1 d g s b NMOD W={w_n} L={l_n}\n\
         .model NMOD NMOS (LEVEL=1 VTO={vto_n} KP={kp_n} LAMBDA={lambda_n} GAMMA={gamma_n} PHI={phi_n})\n\
         .op\n\
         .end\n"
    );

    let (dir_nmos, _) = simulate("nmos_sat", &deck_nmos).await;
    let op_nmos = parse_op_file(&dir_nmos.path.join("sim/nmos_sat/nmos_sat-op.txt"));
    let id_sat_sim = -op_nmos["i(vd)"];
    let rel_err_sat = (id_sat_sim - id_sat_theory).abs() / id_sat_theory;
    assert!(
        rel_err_sat < 1e-4,
        "NMOS saturation Id: sim={id_sat_sim:.6e}, ref={id_sat_theory:.6e}, rel_err={rel_err_sat:.6e}"
    );

    // Part B: Body effect with Vsb = 1.0 V (Source at 0 V, Bulk at -1.0 V)
    // Threshold voltage shift:
    // Vt = VTO + gamma * (sqrt(2*phi_F + Vsb) - sqrt(2*phi_F))
    // where 2*phi_F is the model parameter PHI (0.6 V)
    let vsb: f64 = 1.0;
    let delta_vt: f64 = gamma_n * ((phi_n + vsb).sqrt() - phi_n.sqrt());
    let vt_body: f64 = vto_n + delta_vt;
    let vov_body: f64 = vgs - vt_body;
    let id_body_theory: f64 = (kp_n / 2.0) * (w_n / l_n) * vov_body.powi(2) * (1.0 + lambda_n * vds);

    let deck_body = deck_nmos.replace("Vb b 0 0", "Vb b 0 -1");
    let (dir_body, _) = simulate("nmos_body", &deck_body).await;
    let op_body = parse_op_file(&dir_body.path.join("sim/nmos_body/nmos_body-op.txt"));
    let id_body_sim = -op_body["i(vd)"];
    let rel_err_body = (id_body_sim - id_body_theory).abs() / id_body_theory;
    assert!(
        rel_err_body < 1e-4,
        "NMOS body effect Id: sim={id_body_sim:.6e}, ref={id_body_theory:.6e}, rel_err={rel_err_body:.6e}"
    );

    // Part C: PMOS Current Mirror
    // M1: diode-connected reference PMOS (drain and gate shorted to node 'ref_in')
    // Iref pulls 1 mA from 'ref_in' to ground.
    // M2: mirror PMOS, gate tied to 'ref_in', source tied to supply (5V).
    // Drain of M2 connected to Vload (2V).
    let vdd: f64 = 5.0;
    let vload: f64 = 2.0;
    let iref: f64 = 1e-3;
    let lambda_p: f64 = 0.02;

    let deck_pmos = format!(
        "PMOS Current Mirror\n\
         Vdd supply 0 {vdd}\n\
         Iref ref_in 0 {iref}\n\
         M1 ref_in ref_in supply supply PMOD W=10u L=1u\n\
         M2 out ref_in supply supply PMOD W=10u L=1u\n\
         Vload out 0 {vload}\n\
         .model PMOD PMOS (LEVEL=1 VTO=-1 KP=200u LAMBDA={lambda_p})\n\
         .op\n\
         .end\n"
    );

    let (dir_pmos, _) = simulate("pmos_mirror", &deck_pmos).await;
    let op_pmos = parse_op_file(&dir_pmos.path.join("sim/pmos_mirror/pmos_mirror-op.txt"));

    let v_ref_in = op_pmos["v(ref_in)"];
    let vsd1 = vdd - v_ref_in;
    let vsd2 = vdd - vload;
    // Ratio of currents due to channel-length modulation lambda:
    // Id2 / Id1 = (1 + lambda * Vsd2) / (1 + lambda * Vsd1)
    let id2_theory: f64 = iref * (1.0 + lambda_p * vsd2) / (1.0 + lambda_p * vsd1);
    // Vload branch current: positive into 'out', which equals M2 drain current
    let id2_sim = op_pmos["i(vload)"];
    let rel_err_pmos = (id2_sim - id2_theory).abs() / id2_theory;
    assert!(
        rel_err_pmos < 1e-4,
        "PMOS mirror Id2: sim={id2_sim:.6e}, ref={id2_theory:.6e}, rel_err={rel_err_pmos:.6e}"
    );
}

/// 7. PNP version of a common-emitter bias: Ic matches hand Ebers-Moll within 1 %.
#[tokio::test]
async fn test_7_pnp_common_emitter_bias() {
    let v_ee: f64 = 5.0; // magnitude of supply
    let rb: f64 = 100_000.0;
    let rc: f64 = 500.0;
    let bf: f64 = 100.0;
    let is_bjt: f64 = 1e-15;
    let vt: f64 = 8.617333262145e-5 * 300.15;

    // PNP common-emitter circuit:
    // Emitter at 0 (GND).
    // Supply Vee = -5 V pulls Base through Rb (100k) and Collector through Rc (500).
    // Transistor Q1: collector 'c', base 'b', emitter 0.
    // Forward active mode: E-B junction forward biased with Veb = -Vb > 0.
    let deck = format!(
        "PNP Common-Emitter\n\
         Vee supply 0 -{v_ee}\n\
         Rb supply base {rb}\n\
         Rc supply collector {rc}\n\
         Q1 collector base 0 bjt_pnp\n\
         .model bjt_pnp PNP (IS={is_bjt} BF={bf} VAF=inf)\n\
         .op\n\
         .end\n"
    );

    let (dir, _) = simulate("pnp_ce", &deck).await;
    let op_map = parse_op_file(&dir.path.join("sim/pnp_ce/pnp_ce-op.txt"));

    // Hand Ebers-Moll derivation:
    // Base loop: V_ee = Ib * Rb + Veb = (Ic / BF) * Rb + Vt * ln(1 + Ic / IS)
    // Non-linear equation for Ic: f(Ic) = Ic + (BF * Vt / Rb) * ln(1 + Ic / IS) - (BF * V_ee / Rb) = 0
    // Solve using Newton-Raphson:
    let mut ic_calc: f64 = bf * (v_ee - 0.7) / rb; // initial guess with typical Veb = 0.7 V
    for _ in 0..20 {
        let f: f64 = ic_calc + (bf * vt / rb) * (1.0 + ic_calc / is_bjt).ln() - (bf * v_ee / rb);
        let df: f64 = 1.0 + (bf * vt / rb) / (ic_calc + is_bjt);
        let step: f64 = f / df;
        ic_calc -= step;
        if step.abs() < 1e-12 {
            break;
        }
    }

    // From simulated circuit: Collector current flowing out of Q1 collector into Rc:
    // Ic = (V(collector) - V(supply)) / Rc
    let v_coll = op_map["v(collector)"];
    let v_supp = op_map["v(supply)"];
    let ic_sim = (v_coll - v_supp) / rc;

    let rel_err = (ic_sim - ic_calc).abs() / ic_calc;
    assert!(
        rel_err < 0.01,
        "PNP Ic mismatch: sim={ic_sim:.6e}, calc={ic_calc:.6e}, rel_err={rel_err:.6e} (limit 1 %)"
    );
}

/// 8. `.dc` sweep of a resistor divider: number of rows = (stop-start)/step+1, values linear.
#[tokio::test]
async fn test_8_dc_sweep_resistor_divider() {
    let r1: f64 = 1000.0;
    let r2: f64 = 3000.0;
    let start: f64 = 0.0;
    let stop: f64 = 10.0;
    let step: f64 = 0.5;
    let expected_rows = ((stop - start) / step).round() as usize + 1; // 21 rows

    let deck = format!(
        "Resistor Divider DC Sweep\n\
         V1 in 0 0\n\
         R1 in out {r1}\n\
         R2 out 0 {r2}\n\
         .dc V1 {start} {stop} {step}\n\
         .end\n"
    );

    let (dir, _) = simulate("divider_dc", &deck).await;
    let (headers, rows) = parse_csv_file(&dir.path.join("sim/divider_dc/divider_dc-dc.csv"));

    // Check row count
    assert_eq!(
        rows.len(),
        expected_rows,
        "expected {expected_rows} rows in DC sweep, got {}",
        rows.len()
    );

    let out_col = headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case("v(out)"))
        .expect("v(out) column");

    // Divider ratio: Vout = Vin * (R2 / (R1 + R2)) = Vin * 0.75
    let ratio = r2 / (r1 + r2);

    for (i, row) in rows.iter().enumerate() {
        let expected_vin = start + i as f64 * step;
        let vin = row[0];
        let vout = row[out_col];
        assert!(
            (vin - expected_vin).abs() < 1e-9,
            "row {i}: Vin={vin}, expected={expected_vin}"
        );
        let expected_vout = vin * ratio;
        assert!(
            (vout - expected_vout).abs() < 1e-9,
            "row {i}: Vout={vout}, expected linear value={expected_vout}"
        );
    }

    // Linearity: constant first difference
    let diffs: Vec<f64> = rows.windows(2).map(|w| w[1][out_col] - w[0][out_col]).collect();
    let expected_diff = step * ratio;
    for (i, d) in diffs.iter().enumerate() {
        assert!(
            (d - expected_diff).abs() < 1e-9,
            "step {i}: delta={d}, expected={expected_diff}"
        );
    }
}

/// 9. Netlist syntax: suffixes (`1meg` vs `1m`, `10kohm`, `2.2uF`, `100mil` if supported),
/// `+` continuation, `;` comments, case-insensitivity, `gnd` as ground, `.end` optional —
/// all give the same results as a canonical netlist.
#[tokio::test]
async fn test_9_netlist_syntax_variations() {
    // Canonical netlist with standard numbers and formatting
    let canonical = "\
Canonical Netlist Reference
V1 in 0 10.0
R1 in n1 10000.0
R2 n1 0 10000.0
R3 in n2 1000000.0
R4 n2 0 1000000.0
R5 in n3 0.00254
R6 n3 0 0.00254
R7 in n4 0.001
R8 n4 0 0.001
C1 in 0 2.2e-6
.op
.end
";

    // Variant netlist using:
    // - suffixes: '10kohm' (10k), '1meg' (1e6) vs '1m' (1e-3), '100mil' (2.54e-3), '2.2uF' (2.2u)
    // - '+' continuation lines
    // - ';' inline and trailing comments
    // - case-insensitivity ('v1', 'R1', 'IN', 'gnd', 'GND', '.OP')
    // - 'gnd' as ground
    // - '.end' omitted
    let variant = "\
Variant Netlist with Suffixes and Syntactic Sugar
v1 IN gnd 10V ; voltage source with V unit and gnd
r1 in n1 10kohm ; 10kohm suffix
+
r2 N1 GND 10kohm ; 10kohm suffix, uppercase N1 and GND
; comment line
r3 in n2
+ 1meg ; continuation with 1meg suffix
r4 N2 gnd 1meg ; 1meg suffix
r5 IN n3 100mil ; 100mil suffix
r6 n3 GND 100mil
r7 in n4 1m ; 1m suffix (1 milli-ohm)
r8 N4 gnd 1m ; 1m suffix
c1 in 0 2.2uF ; 2.2uF capacitor
.OP
";

    let (dir_c, _) = simulate("syntax_canonical", canonical).await;
    let (dir_v, _) = simulate("syntax_variant", variant).await;

    let op_c = parse_op_file(&dir_c.path.join("sim/syntax_canonical/syntax_canonical-op.txt"));
    let op_v = parse_op_file(&dir_v.path.join("sim/syntax_variant/syntax_variant-op.txt"));

    for key in ["v(in)", "v(n1)", "v(n2)", "v(n3)", "v(n4)", "i(v1)"] {
        let val_c = op_c[key];
        let val_v = op_v[key];
        let diff = (val_c - val_v).abs();
        let tol = 1e-9 * val_c.abs().max(1.0);
        assert!(
            diff < tol,
            "Syntax variation mismatch for {key}: canonical={val_c}, variant={val_v}"
        );
    }

    // Explicit check that each equal divider gave 5.0 V:
    for node in ["v(n1)", "v(n2)", "v(n3)", "v(n4)"] {
        assert!(
            (op_v[node] - 5.0).abs() < 1e-9,
            "{node} should be 5.0 V, got {}",
            op_v[node]
        );
    }

    // Explicit check for 1meg vs 1m:
    // If 1meg was parsed as 1m in R3, then R3=1m and R4=1meg, giving V(n2) ≈ 10.0 V, not 5.0 V.
    // Let's verify that a divider with 1m and 1meg produces near-rail voltage:
    let deck_1m_vs_1meg = "\
Divider 1m vs 1meg
V1 in 0 10.0
R1 in n 1m
R2 n 0 1meg
.op
.end
";
    let (dir_m, _) = simulate("m_vs_meg", deck_1m_vs_1meg).await;
    let op_m = parse_op_file(&dir_m.path.join("sim/m_vs_meg/m_vs_meg-op.txt"));
    // With R1=1e-3 and R2=1e6: V(n) = 10 * 1e6 / (1e6 + 1e-3) ≈ 10.0 V - 1e-8 V
    assert!(
        (op_m["v(n)"] - 10.0).abs() < 1e-6,
        "1m vs 1meg divider expected ~10.0 V, got {}",
        op_m["v(n)"]
    );
}

/// 10. Every error message in the brief, one assertion each (parse errors with correct 1-based
/// line numbers, floating node, voltage source loop including two parallel V sources,
/// missing analysis, wrong extension, outside-project path).
#[tokio::test]
async fn test_10_all_contract_error_messages() {
    async fn check_err(source: &str, files: &[(&str, &str)], expected_err: &str) {
        let dir = support::project("err_test", files);
        let err = support::run("circuit_simulate", json!({"source": source}), &dir.path)
            .await
            .expect_err("should return an error")
            .to_string();
        assert_eq!(
            err, expected_err,
            "Error mismatch for source '{source}'. Expected '{expected_err}', got '{err}'"
        );
    }

    // 1. Wrong extension
    check_err(
        "sim/test.txt",
        &[("sim/test.txt", "Title\n.op\n")],
        "source must be a .cir file",
    ).await;

    // 2. Non-existent file
    check_err(
        "sim/missing.cir",
        &[],
        "sim/missing.cir does not exist; write it with write_file first",
    ).await;

    // 3. Outside-project path
    check_err(
        "../outside.cir",
        &[],
        "path \"../outside.cir\" is outside the project",
    ).await;

    // 4. Parse error: unknown element with 1-based line number (line 2)
    check_err(
        "sim/unknown_elem.cir",
        &[("sim/unknown_elem.cir", "Title\nU1 1 0 1k\n.op\n")],
        "line 2: unknown element U1",
    ).await;

    // 5. Parse error: needs k nodes with 1-based line number (line 2)
    check_err(
        "sim/needs_nodes.cir",
        &[("sim/needs_nodes.cir", "Title\nR1 1 1k\n.op\n")],
        "line 2: R1 needs 2 nodes",
    ).await;

    // 6. Parse error: bad number with 1-based line number (line 2)
    check_err(
        "sim/bad_num.cir",
        &[("sim/bad_num.cir", "Title\nR1 1 0 bad_val\n.op\n")],
        "line 2: bad number bad_val",
    ).await;

    // 7. Parse error: unknown model with 1-based line number (line 2)
    check_err(
        "sim/unknown_model.cir",
        &[("sim/unknown_model.cir", "Title\nD1 1 0 UNKNOWN_MOD\n.op\n")],
        "line 2: unknown model UNKNOWN_MOD",
    ).await;

    // 8. Parse error: unknown subcircuit with 1-based line number (line 2)
    check_err(
        "sim/unknown_sub.cir",
        &[("sim/unknown_sub.cir", "Title\nX1 1 0 UNKNOWN_SUB\n.op\n")],
        "line 2: unknown subcircuit UNKNOWN_SUB",
    ).await;

    // 9. Missing analysis directive
    check_err(
        "sim/no_analysis.cir",
        &[("sim/no_analysis.cir", "Title\nR1 1 0 1k\n")],
        "no analysis: add .op, .dc or .tran",
    ).await;

    // 10. Floating node (no DC path to ground)
    check_err(
        "sim/floating.cir",
        &[("sim/floating.cir", "Title\nI1 1 0 1m\nC1 1 0 1u\n.op\n")],
        "node 1 has no DC path to ground",
    ).await;

    // 11. Voltage source loop including two parallel V sources
    check_err(
        "sim/v_loop.cir",
        &[("sim/v_loop.cir", "Title\nV1 1 0 5\nV2 1 0 5\n.op\n")],
        "voltage source loop: V1, V2",
    ).await;
}

/// 11. Determinism: two runs give byte-identical output files.
/// SVG starts with `<svg` and contains a legend entry per trace.
#[tokio::test]
async fn test_11_determinism_and_svg_structure() {
    let deck = "\
Determinism and Plot Deck
V1 in 0 5
R1 in out 1k
C1 out 0 1u
.op
.dc V1 0 5 1
.tran 10u 100u
.plot tran v(in) v(out)
.plot dc v(in) v(out)
.end
";

    let (dir1, _) = simulate("det_run", deck).await;
    let (dir2, _) = simulate("det_run", deck).await;

    let files_to_compare = [
        "sim/det_run/det_run-op.txt",
        "sim/det_run/det_run-dc.csv",
        "sim/det_run/det_run-dc.svg",
        "sim/det_run/det_run-tran.csv",
        "sim/det_run/det_run-tran.svg",
    ];

    for file in &files_to_compare {
        let bytes1 = fs::read(dir1.path.join(file))
            .unwrap_or_else(|e| panic!("failed to read {file} from run 1: {e}"));
        let bytes2 = fs::read(dir2.path.join(file))
            .unwrap_or_else(|e| panic!("failed to read {file} from run 2: {e}"));
        assert_eq!(
            bytes1, bytes2,
            "File {file} is not byte-identical between two consecutive runs"
        );
    }

    // SVG validation
    for svg_file in ["sim/det_run/det_run-dc.svg", "sim/det_run/det_run-tran.svg"] {
        let content = fs::read_to_string(dir1.path.join(svg_file)).expect("read svg");
        assert!(
            content.starts_with("<svg"),
            "{svg_file} must start with '<svg', got: {}",
            &content[..content.len().min(50)]
        );

        // Check legend entry per plotted trace: 'v(in)' and 'v(out)'
        for trace in ["v(in)", "v(out)"] {
            assert!(
                content.contains(&format!(">{trace}<")),
                "{svg_file} must contain a legend entry for trace '{trace}'"
            );
        }
    }
}
