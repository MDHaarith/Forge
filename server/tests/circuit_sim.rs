mod common;
use common::TempDir;
use forge_server::tools::{all_tools, circuit::{simulate, Simulation}, ToolContext};
use serde_json::{json, Value};
use std::{collections::BTreeMap, time::Instant};

fn fixture(name: &str) -> Value {
    let text = match name {
        "linear" => include_str!("fixtures/sim/linear.json"),
        "reactive" => include_str!("fixtures/sim/reactive.json"),
        "semiconductors" => include_str!("fixtures/sim/semiconductors.json"),
        _ => unreachable!(),
    };
    let value: Value = serde_json::from_str(text).unwrap();
    assert!(!value["source"].as_str().unwrap().is_empty()); value
}
fn file<'a>(result: &'a Simulation, suffix: &str) -> &'a str { &result.files.iter().find(|(p, _)| p.ends_with(suffix)).unwrap().1 }
fn op(netlist: &str) -> BTreeMap<String, f64> {
    let result = simulate(netlist, "test").unwrap();
    file(&result, "-op.txt").lines().map(|s| {
        let (label, value) = s.split_once(" = ").unwrap();
        (label.to_ascii_lowercase(), value.split_whitespace().next().unwrap().parse().unwrap())
    }).collect()
}
fn csv(result: &Simulation, kind: &str) -> (Vec<String>, Vec<Vec<f64>>) {
    let content = file(result, &format!("-{kind}.csv"));
    let mut lines = content.lines();
    let headers = lines.next().unwrap().split(',').map(str::to_owned).collect();
    let rows = lines.map(|s| s.split(',').map(|v| v.parse::<f64>().unwrap()).collect()).collect();
    (headers, rows)
}
fn at(headers: &[String], rows: &[Vec<f64>], column: &str, time: f64) -> f64 {
    let k = headers.iter().position(|h| h.eq_ignore_ascii_case(column)).unwrap();
    let upper = rows.partition_point(|r| r[0] < time);
    if upper == 0 { return rows[0][k]; }
    if upper == rows.len() { return rows.last().unwrap()[k]; }
    let (a, b) = (&rows[upper - 1], &rows[upper]);
    a[k] + (b[k] - a[k]) * (time - a[0]) / (b[0] - a[0])
}
fn accuracy(test: &str, reference: f64, result: f64, absolute_limit: f64) {
    let error = (result - reference).abs();
    println!("accuracy | {test} | reference {reference:.12e} | result {result:.12e} | abs error {error:.6e} | relative {:.6e}", error / reference.abs().max(1e-300));
    assert!(error < absolute_limit, "{test}: got {result}, expected {reference}, error {error}, limit {absolute_limit}");
}

#[test]
fn divider_and_wheatstone_bridge_match_exact_nodal_solutions() {
    let f = fixture("linear");
    let v = op("divider\nV1 in 0 DC 10V\nR1 IN out 1kohm\nR2 out GND 3K\n.op\n.end\n");
    accuracy("divider", f["divider_v"].as_f64().unwrap(), v["v(out)"], 1e-9);
    let v = op("bridge\nV1 in 0 10\nR1 in a 1k\nR2 a 0 2k\nR3 in b 3k\nR4 b 0 4k\nR5 a b 5k\n.op\n");
    accuracy("bridge a", f["bridge_a_v"].as_f64().unwrap(), v["v(a)"], 1e-9);
    accuracy("bridge b", f["bridge_b_v"].as_f64().unwrap(), v["v(b)"], 1e-9);
}

#[test]
fn rc_and_rl_steps_and_underdamped_rlc_match_analytic_responses() {
    let f = fixture("reactive");
    for (label, netlist, column, reference, scale) in [
        ("RC", "RC step\nV1 in 0 PWL(0 0 1n 5 10m 5)\nR1 in out 1k\nC1 out 0 1uF\n.tran 10u 5m\n", "v(out)", "rc_v", 5.),
        ("RL", "RL step\nV1 in 0 PWL(0 0 1n 5 10m 5)\nR1 in out 1k\nL1 out 0 1H\n.tran 10u 5m\n", "i(L1)", "rl_a", 0.005),
    ] {
        let result = simulate(netlist, "test").unwrap(); let (headers, rows) = csv(&result, "tran");
        for (i, time) in f["times_s"].as_array().unwrap().iter().enumerate() {
            let time = time.as_f64().unwrap();
            accuracy(&format!("{label} t={time}"), f[reference][i].as_f64().unwrap(), at(&headers, &rows, column, time), scale * 0.001);
        }
        assert_eq!(rows.last().unwrap()[0], 0.005);
    }
    let result = simulate("RLC\nV1 in 0 PWL(0 0 1n 1 2m 1)\nR1 in a 20\nL1 a out 10m\nC1 out 0 1u\n.tran 1u 1m\n", "test").unwrap();
    let (headers, rows) = csv(&result, "tran");
    let k = headers.iter().position(|s| s == "v(out)").unwrap();
    let peak = rows.iter().max_by(|a, b| a[k].total_cmp(&b[k])).unwrap();
    let tp = f["rlc_peak_time_s"].as_f64().unwrap(); let vp = f["rlc_peak_v"].as_f64().unwrap();
    accuracy("RLC peak time", tp, peak[0], tp * 0.005);
    accuracy("RLC overshoot", vp - 1., peak[k] - 1., (vp - 1.) * 0.005);
    let coarse = "adaptive RC\nV1 in 0 PWL(0 0 0 5)\nR1 in out 1k\nC1 out 0 1u\n.tran 1m 5m 0 1m\n";
    let loose = simulate(coarse, "test").unwrap(); let (_, loose_rows) = csv(&loose, "tran");
    let tight = simulate(&format!("{coarse}.options reltol=1e-6 vntol=1e-9\n"), "test").unwrap();
    let (headers, tight_rows) = csv(&tight, "tran");
    assert!(tight_rows.len() > 2 * loose_rows.len());
    accuracy("adaptive RC coarse tmax", f["rc_v"][2].as_f64().unwrap(), at(&headers, &tight_rows, "v(out)", 0.001), 5. * 0.001);
}

#[test]
fn diode_lambert_w_and_sinusoidal_rectifier() {
    let f = fixture("semiconductors");
    let v = op("diode\nV1 in 0 5\nR1 in out 1k\nD1 out 0 diode\n.model diode D(IS=1e-14 N=1)\n.op\n");
    let reference = f["diode_current_a"].as_f64().unwrap();
    accuracy("diode Lambert W", reference, -v["i(v1)"], reference * 1e-6);
    let stepped = op("diode stepping\nV1 in 0 5\nR1 in out 1k\nD1 out 0 d\n.model d D(IS=1e-14 N=1)\n.options itl1=6\n.op\n");
    accuracy("diode convergence stepping", reference, -stepped["i(v1)"], reference * 1e-6);
    let result = simulate("rectifier\nV1 in 0 SIN(0 5 1k 0 0)\nD1 in out rect\nR1 out 0 1k\n.model rect D(IS=1e-14 N=1)\n.tran 1u 1m\n", "test").unwrap();
    let (headers, rows) = csv(&result, "tran");
    let k = headers.iter().position(|s| s == "v(out)").unwrap();
    let peak = rows.iter().map(|r| r[k]).fold(f64::NEG_INFINITY, f64::max);
    accuracy("SIN half-wave peak", f["rectifier_peak_v"].as_f64().unwrap(), peak, 0.02);
    assert!(at(&headers, &rows, "v(out)", 0.00075).abs() < 1e-6);
}

#[test]
fn npn_pnp_bias_and_early_effect() {
    let f = fixture("semiconductors");
    let deck = "NPN\nVcc supply 0 5\nRb supply base 100k\nRc supply collector 500\nQ1 collector base 0 bjt\n.model bjt NPN(IS=1e-15 BF=100 VAF=inf)\n.op\n";
    let v = op(deck);
    let ic = (5. - v["v(collector)"]) / 500.;
    accuracy("NPN Ic", f["npn_ic_a"].as_f64().unwrap(), ic, ic * 0.005);
    accuracy("NPN Vce", f["npn_vce_v"].as_f64().unwrap(), v["v(collector)"], v["v(collector)"] * 0.005);
    let early = op(&deck.replace("VAF=inf", "VAF=50"));
    assert!(early["v(collector)"] < v["v(collector)"]);
    let pnp = op(&deck.replace("NPN(IS", "PNP(IS").replace("supply 0 5", "supply 0 -5"));
    accuracy("PNP symmetry", -v["v(collector)"], pnp["v(collector)"], 1e-9);
}

#[test]
fn mos_square_law_triode_body_effect_and_cmos_thresholds() {
    let f = fixture("semiconductors");
    let deck = "MOS\nVd d 0 5\nVg g 0 3\nVb b 0 0\nM1 d g 0 b nf W=10u L=1u\n.model nf NMOS(LEVEL=1 VTO=1 KP=200u LAMBDA=.02 GAMMA=.5 PHI=.6)\n.op\n";
    for (name, deck, reference) in [("MOS saturation", deck.to_owned(), "mos_saturation_a"), ("MOS triode", deck.replace("Vd d 0 5", "Vd d 0 .5"), "mos_triode_a")] {
        let v = op(&deck); let reference = f[reference].as_f64().unwrap();
        accuracy(name, reference, -v["i(vd)"], reference * 1e-6);
    }
    let reverse_body = op(&deck.replace("Vb b 0 0", "Vb b 0 -1"));
    let reference = f["mos_body_effect_a"].as_f64().unwrap();
    accuracy("MOS body effect", reference, -reverse_body["i(vd)"], reference * 1e-6);
    for (width, reference) in [(2, "cmos_matched_vm_v"), (4, "cmos_mismatched_vm_v")] {
        let deck = format!("CMOS\nVdd supply 0 5\nVin in 0 0\nMn out in 0 0 nf W=1u L=1u\nMp out in supply supply pf W={width}u L=1u\n.model nf NMOS(VTO=1 KP=200u LAMBDA=.001)\n.model pf PMOS(VTO=-1 KP=100u LAMBDA=.001)\n.options reltol=1e-6 vntol=1e-9\n.dc Vin 0 5 .01\n.plot dc v(out)\n");
        let result = simulate(&deck, "test").unwrap(); let (headers, rows) = csv(&result, "dc");
        let out = headers.iter().position(|s| s == "v(out)").unwrap();
        assert!(rows.windows(2).all(|p| p[1][out] <= p[0][out] + 1e-8));
        let crossing = rows.windows(2).find(|p| p[0][out] >= p[0][0] && p[1][out] <= p[1][0]).unwrap();
        let (a, b) = (&crossing[0], &crossing[1]);
        let da = a[out] - a[0]; let db = b[out] - b[0];
        let vm = a[0] + (b[0] - a[0]) * da / (da - db);
        accuracy(&format!("CMOS Wp={width} Vm"), f[reference].as_f64().unwrap(), vm, 0.01);
    }
}

#[test]
fn diode_series_resistance_reverse_breakdown_and_charge_storage() {
    let f = fixture("semiconductors");
    let forward = op("diode RS\nV1 in 0 5\nR1 in out 1k\nD1 out 0 d\n.model d D(IS=1e-14 RS=100)\n.op\n");
    let reference = f["diode_rs_current_a"].as_f64().unwrap();
    accuracy("diode RS", reference, -forward["i(v1)"], reference * 1e-6);
    let reverse = op("zener\nV1 in 0 -10\nR1 in out 1k\nD1 out 0 d\n.model d D(IS=1e-14 RS=10 BV=5.1 IBV=1m)\n.op\n");
    let reference = f["zener_current_a"].as_f64().unwrap();
    accuracy("diode BV/IBV", reference, -reverse["i(v1)"], reference.abs() * 1e-6);
    let charged = simulate("diode stored charge\nV1 out 0 PWL(0 .4 10u .5)\nD1 out 0 d\n.model d D(IS=1p N=1 CJO=1n VJ=1 M=.5 TT=1u)\n.tran 10n 10u\n", "test").unwrap();
    let (headers, rows) = csv(&charged, "tran");
    let reference = f["diode_charge_current_a"].as_f64().unwrap();
    accuracy("diode CJO/TT", reference, -at(&headers, &rows, "i(V1)", 5e-6), reference * 0.001);
}

#[test]
fn gummel_poon_parameters_parasitic_resistors_and_forward_reverse_charge() {
    let f = fixture("semiconductors");
    let gp = op("Gummel Poon\nVc c 0 2\nVb b 0 .7\nQ1 c b 0 q\n.model q NPN(IS=1e-15 BF=100 BR=4 NF=1.1 NR=1.2 VAF=50 VAR=30 IKF=1m)\n.op\n");
    let reference = f["gummel_poon_ic_a"].as_f64().unwrap();
    accuracy("Gummel-Poon", reference, -gp["i(vc)"], reference * 1e-6);
    let parasitic = op("parasitics\nV1 supply 0 5\nRb supply b 100k\nRc supply c 500\nQ1 c b 0 q\n.model q NPN(IS=1e-15 BF=100 RB=100 RC=10 RE=5)\n.op\n");
    let flat = op("flat parasitics\nV1 supply 0 5\nRb supply b 100k\nRc supply c 500\nRbi b bi 100\nRci c ci 10\nRei ei 0 5\nQ1 ci bi ei q\n.model q NPN(IS=1e-15 BF=100)\n.op\n");
    for column in ["v(b)", "v(c)", "i(v1)"] { accuracy("BJT RB/RC/RE", flat[column], parasitic[column], 1e-10); }
    for (collector, emitter, reference) in [(1, 0, "bjt_forward_base_current_a"), (0, 1, "bjt_reverse_base_current_a")] {
        let deck = format!("BJT charge\nVc c 0 {collector}\nVe e 0 {emitter}\nVb b 0 PWL(0 .5 1u .6)\nQ1 c b e q\n.model q NPN(IS=1e-15 BF=100 BR=5 CJE=1n CJC=2n TF=100n TR=1u)\n.tran 1n 1u\n");
        let result = simulate(&deck, "test").unwrap(); let (headers, rows) = csv(&result, "tran");
        let reference_value = f[reference].as_f64().unwrap();
        accuracy(reference, reference_value, -at(&headers, &rows, "i(Vb)", 0.5e-6), reference_value * 0.001);
    }
}

#[test]
fn mos_overlap_and_bulk_capacitance_currents_match_charge_derivatives() {
    let f = fixture("semiconductors");
    for (d, g, s, sense, reference) in [
        (".2", "PWL(0 .1 1u .2)", "0", "i(Vg)", "mos_gate_charge_current_a"),
        ("PWL(0 .1 1u .2)", "0", "0", "i(Vd)", "mos_drain_charge_current_a"),
        ("0", "0", "PWL(0 .1 1u .2)", "i(Vs)", "mos_source_charge_current_a"),
    ] {
        let deck = format!("MOS charge\nVd d 0 {d}\nVg g 0 {g}\nVs s 0 {s}\nVb b 0 -.5\nM1 d g s b n W=10u L=1u\n.model n NMOS(VTO=1 CGSO=1m CGDO=2m CBD=1n CBS=2n)\n.tran 1n 1u\n");
        let result = simulate(&deck, "test").unwrap(); let (headers, rows) = csv(&result, "tran");
        let reference_value = f[reference].as_f64().unwrap();
        accuracy(reference, reference_value, -at(&headers, &rows, sense, 0.5e-6), reference_value * 0.001);
    }
}

#[test]
fn nested_subcircuits_local_models_and_controlled_sources() {
    let nested = "nested\nV1 in 0 6\nX1 in out 0 wrapper\n.subckt wrapper p q g\nX2 p q g divider\n.ends\n.subckt divider a b g\nR1 a b 1k\nR2 b g 2k\n.ends\n.op\n";
    let flat = "flat\nV1 in 0 6\nR1 in out 1k\nR2 out 0 2k\n.op\n";
    let (n, f) = (op(nested), op(flat));
    accuracy("nested subcircuit", f["v(out)"], n["v(out)"], 1e-12);
    accuracy("nested source current", f["i(v1)"], n["i(v1)"], 1e-12);
    let local = op("local model\nV1 in 0 1\nX1 in out rect\n.subckt rect a b\nD1 a b dm\nR1 b 0 1k\n.model dm D(IS=1e-14)\n.ends rect\n.op\n");
    assert!(local["v(out)"] > 0.3 && local["v(out)"] < 0.5);
    let controlled = op("ideal amplifier\nVin in 0 .1\nRi in inv 1k\nRf inv out 10k\nEamp out 0 0 inv 1e10\nVcontrol control 0 2\nG1 load 0 control 0 2m\nRload load 0 1k\n.op\n");
    accuracy("VCVS gain", -10., controlled["v(out)"] / 0.1, 1e-6);
    accuracy("VCCS", -4., controlled["v(load)"], 1e-12);
}

#[test]
fn spice_syntax_initial_conditions_waveforms_options_and_plots() {
    let syntax = "title ignored even when it looks like R1 0 1 3\n* comment\nv1 in gnd dc 5V ; tail\nr1 IN mid 1MEGohm\n+ ; continuation across a comment\nr2 mid GND 1meg\n.OP\n.END\nZbad x y 1\n";
    accuracy("suffix/case/continuation", 2.5, op(syntax)["v(mid)"], 1e-12);
    for (suffix, expected) in [("f", 1e-15), ("p", 1e-12), ("n", 1e-9), ("u", 1e-6), ("m", 1e-3), ("k", 1e3), ("meg", 1e6), ("g", 1e9), ("t", 1e12), ("mil", 25.4e-6)] {
        let v = op(&format!("suffix\nV1 in 0 1\nR1 in 0 1{suffix}\n.op\n"));
        accuracy(&format!("suffix {suffix}"), 1. / expected, -v["i(v1)"], 1e-12 / expected);
    }
    for (deck, column, initial, end) in [
        ("IC cap\nR1 out 0 1k\nC1 out 0 1u ic=2\n.tran 10u 1m uic\n", "v(out)", 2., 2. / std::f64::consts::E),
        ("IC node\nR1 out 0 1k\nC1 out 0 1u\n.ic v(out)=2\n.tran 10u 1m\n", "v(out)", 2., 2. / std::f64::consts::E),
        ("IC inductor\nR1 out 0 1k\nL1 out 0 1 ic=2m\n.tran 10u 1m uic\n", "i(L1)", 0.002, 0.002 / std::f64::consts::E),
    ] {
        let result = simulate(deck, "test").unwrap(); let (headers, rows) = csv(&result, "tran");
        accuracy("initial condition t=0", initial, at(&headers, &rows, column, 0.), initial * 1e-10);
        accuracy("initial condition decay", end, at(&headers, &rows, column, 0.001), initial * 0.001);
    }
    let pulse = simulate("Pulse <script> & title\nV1 out 0 PULSE(0 2 10u 1u 2u 20u 50u)\nR1 out 0 1k\n.tran 3u 100u 5u 3u\n.print tran v(out) i(V1)\n.options reltol=.0001 abstol=1p vntol=1u gmin=1p itl1=100 itl4=20\n", "test").unwrap();
    let (headers, rows) = csv(&pulse, "tran");
    for corner in [0.00001, 0.000011, 0.000031, 0.000033, 0.00006] { assert!(rows.iter().any(|r| (r[0] - corner).abs() < 1e-15), "missing breakpoint {corner}"); }
    assert_eq!(rows[0][0], 5e-6);
    accuracy("PULSE high", 2., at(&headers, &rows, "v(out)", 15e-6), 1e-12);
    let svg = file(&pulse, "-tran.svg");
    assert!(svg.contains("&lt;script&gt; &amp; title")); assert!(!svg.contains("<script>"));
    assert!(svg.contains("i(V1)")); assert!(svg.contains("time (s)")); assert!(svg.contains("polyline"));
    let current = simulate("I source\nI1 0 out DC 1m PWL(0 0 1m 2m 2m 0)\nR1 out 0 1k\n.op\n.dc I1 2m 0 -1m\n.tran 10u 2m\n", "test").unwrap();
    let (headers, rows) = csv(&current, "dc");
    assert_eq!(rows.len(), 3); accuracy("current DC sweep", 2., at(&headers, &rows[..1], "v(out)", 0.), 1e-12);
    let (headers, rows) = csv(&current, "tran"); accuracy("current PWL", 2., at(&headers, &rows, "v(out)", 0.001), 1e-12);
    assert_eq!(rows[0][headers.iter().position(|s| s == "v(out)").unwrap()], 0.);
    let defaults = simulate("SPICE defaults\nV1 out 0 DC 7 SIN(0 2 0)\nR1 out 0 1k\n.op\n.tran 10u 1m\n.plot tran v(gnd)\n", "test").unwrap();
    assert!(file(&defaults, "-op.txt").contains("v(out) = 7 V"));
    let (headers, rows) = csv(&defaults, "tran");
    accuracy("SIN zero-frequency default", 2., at(&headers, &rows, "v(out)", 0.00025), 0.001);
    assert!(file(&defaults, "-tran.svg").contains("v(0)"));
    let instant = simulate("PWL duplicate corner\nV1 in 0 PWL(0 0 0 1)\nR1 in out 1k\nC1 out 0 1u\n.tran 10u 1m\n", "test").unwrap();
    let (headers, rows) = csv(&instant, "tran");
    accuracy("instant PWL RC", 1. - 1. / std::f64::consts::E, at(&headers, &rows, "v(out)", 0.001), 0.001);
}

#[test]
fn exact_errors_and_resource_guards() {
    for (deck, error) in [
        ("bad\nZ1 1 0 1\n.op", "line 2: unknown element Z1"),
        ("bad\nR1 1 0\n.op", "line 2: R1 needs 2 nodes"),
        ("bad\nR1 1 0 nope\n.op", "line 2: bad number nope"),
        ("bad\nD1 1 0 absent\n.op", "line 2: unknown model absent"),
        ("bad\nX1 1 0 absent\n.op", "line 2: unknown subcircuit absent"),
        ("bad\nR1 1 0 1k", "no analysis: add .op, .dc or .tran"),
        ("bad\nC1 lone 0 1u\n.op", "node lone has no DC path to ground"),
        ("bad\nV1 1 0 1\nV2 1 0 2\n.op", "voltage source loop: V1, V2"),
        ("bad\nV1 in 0 5\nR1 in out 1k\nD1 out 0 d\n.model d D\n.options itl1=1\n.op", "operating point did not converge (gmin and source stepping failed)"),
        ("bad\nR1 out 0 1k\nC1 out 0 1u\n.tran 1e-20 1u", "timestep too small at t=0"),
    ] { assert_eq!(simulate(deck, "test").err().unwrap(), error); }
    let mut big = "big\n".to_owned(); for i in 1..=2001 { big.push_str(&format!("R{i} n{i} 0 1k\n")); } big.push_str(".op\n");
    assert_eq!(simulate(&big, "test").err().unwrap(), "circuit too large: 2001 unknowns (limit 2000)");
    let recursive = "recursive\nX1 a loop\n.subckt loop p\nXagain p loop\n.ends\n.op\n";
    assert_eq!(simulate(recursive, "test").err().unwrap(), "subcircuit nesting too deep (limit 32)");
    for deck in ["bad\nR1 out 0 NaN\n.op", "bad\nV1 out 0 PWL(1 2 0 3)\n.op", "bad\n.model n NMOS(level=2)\n.op", "bad\n.include /etc/passwd\n.op", "bad\n()\n.op", "bad\n+ missing predecessor\n.op", "bad\n.ic v(out)=\n.op"] { assert!(simulate(deck, "test").is_err()); }
}

#[tokio::test]
async fn tool_registry_path_rules_outputs_and_determinism() {
    let dir = TempDir::new("circuit"); let tool = all_tools().into_iter().find(|t| t.name == "circuit_simulate").unwrap();
    assert_eq!(tool.domain, "pcb"); assert!(tool.requires.is_empty()); assert!(tool.description.lines().count() <= 6);
    let context = || ToolContext::new(dir.path.clone(), dir.path.clone());
    for (source, error) in [("../outside.cir", "path \"../outside.cir\" is outside the project"), ("/tmp/outside.cir", "path \"/tmp/outside.cir\" is outside the project"), (".git/x.cir", "the .git folder is managed by the git tools"), ("sim/x.txt", "source must be a .cir file"), ("sim/missing.cir", "sim/missing.cir does not exist; write it with write_file first")] {
        assert_eq!((tool.run)(json!({"source":source}), context()).await.err().unwrap().to_string(), error);
    }
    std::fs::create_dir_all(dir.path.join("sim")).unwrap();
    std::fs::write(dir.path.join("sim/big.cir"), "*".repeat(1_048_577)).unwrap();
    assert_eq!((tool.run)(json!({"source":"sim/big.cir"}), context()).await.err().unwrap().to_string(), "netlist too large (limit 1 MiB)");
    let deck = "determinism\nV1 in 0 PULSE(0 1 0 1u 1u 1m 2m)\nR1 in out 1k\nC1 out 0 1u\n.op\n.dc V1 0 1 .1\n.tran 10u 1m\n";
    std::fs::write(dir.path.join("sim/test.cir"), deck).unwrap();
    let first = (tool.run)(json!({"source":"sim/test.cir"}), context()).await.unwrap();
    let bytes: Vec<_> = first.files.iter().map(|p| std::fs::read(dir.path.join(p)).unwrap()).collect();
    let second = (tool.run)(json!({"source":"sim/test.cir"}), context()).await.unwrap();
    assert_eq!(first.text, second.text); assert_eq!(first.files, second.files);
    assert_eq!(first.files, ["sim/test/test-op.txt", "sim/test/test-dc.csv", "sim/test/test-dc.svg", "sim/test/test-tran.csv", "sim/test/test-tran.svg"]);
    for (path, bytes) in second.files.iter().zip(bytes) { assert_eq!(std::fs::read(dir.path.join(path)).unwrap(), bytes); }
    let mut large = "table\n".to_owned(); for i in 1..=45 { large.push_str(&format!("R{i} n{i} 0 1k\n")); } large.push_str(".op\n");
    let result = simulate(&large, "table").unwrap(); assert!(result.text.contains("... (full table in -op.txt)")); assert_eq!(file(&result, "-op.txt").lines().count(), 46);
}

#[test]
fn two_hundred_node_rc_ladder_one_thousand_steps_under_five_seconds_debug() {
    let mut deck = "RC ladder\nV1 in 0 PWL(0 0 1n 1 1 1)\nR1 in n1 1k\nC1 n1 0 1n\n".to_owned();
    for i in 2..=200 { deck.push_str(&format!("R{i} n{} n{i} 1k\nC{i} n{i} 0 1n\n", i - 1)); }
    deck.push_str(".tran 1u 1m 0 1u\n.print tran v(n200)\n");
    let started = Instant::now(); let result = simulate(&deck, "ladder").unwrap(); let elapsed = started.elapsed();
    let (_, rows) = csv(&result, "tran"); assert!(rows.len() >= 1000);
    println!("performance | 200-node RC ladder | {} accepted points | {:.3} s debug", rows.len(), elapsed.as_secs_f64());
    assert!(elapsed.as_secs_f64() < 5., "200-node ladder took {elapsed:?}");
}
