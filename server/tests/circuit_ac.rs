mod common;
use common::TempDir;
use forge_server::tools::{all_tools, circuit::{simulate, Simulation}, ToolContext};
use serde_json::json;

fn csv<'a>(result: &'a Simulation, suffix: &str) -> (Vec<&'a str>, Vec<Vec<f64>>) {
    let mut lines = result.files.iter().find(|(p, _)| p.ends_with(suffix)).unwrap().1.lines();
    let headers = lines.next().unwrap().split(',').collect();
    let rows = lines.map(|line| line.split(',').map(|s| s.parse().unwrap()).collect()).collect();
    (headers, rows)
}
fn close(actual: f64, expected: f64, tolerance: f64) { assert!((actual - expected).abs() <= tolerance, "got {actual}, expected {expected}"); }

#[test]
fn frequency_grids_complex_sources_and_bode_output() {
    for (mode, n, stop, expected) in [("dec", 2, 100., vec![1., 10f64.sqrt(), 10., 1000f64.sqrt(), 100.]), ("oct", 1, 8., vec![1., 2., 4., 8.]), ("lin", 3, 5., vec![1., 3., 5.]), ("lin", 1, 5., vec![1.]), ("dec", 10, 1., vec![1.])] {
        let deck = format!("AC <script> & title\nV1 in 0 DC 2 AC 2 90\nR1 in out 1k\nR2 out 0 1k\n.ac {mode} {n} 1 {stop}\n.plot ac v(out) v(gnd)\n");
        let result = simulate(&deck, "grid").unwrap();
        let (headers, rows) = csv(&result, "-ac.csv");
        assert_eq!(headers, ["freq", "vm(in)", "vp(in)", "vm(out)", "vp(out)"]);
        assert_eq!(rows.len(), expected.len());
        for (row, frequency) in rows.iter().zip(expected) { close(row[0], frequency, frequency * 1e-11); close(row[3], 1., 1e-12); close(row[4], 90., 1e-12); }
        let svg = &result.files.iter().find(|(p, _)| p.ends_with("-ac.svg")).unwrap().1;
        for label in ["magnitude (dB)", "phase (degrees)", "frequency (Hz, log scale)", "&lt;script&gt; &amp; title"] { assert!(svg.contains(label)); }
        assert!(!svg.contains("NaN") && !svg.contains("<script>"));
    }
    for source in ["AC 1 PWL(0 0 1m 1)", "SIN(0 1 1k) AC 1", "DC 0 AC 1", "AC 1 DC 0"] {
        let result = simulate(&format!("source order\nI1 0 out {source}\nR1 out 0 1k\n.ac lin 2 1 2\n"), "order").unwrap();
        let (_, rows) = csv(&result, "-ac.csv"); close(rows[0][1], 1000., 1e-9);
    }
    let result = simulate("silent DC source\nV1 in 0 10\nR1 in 0 1k\n.ac lin 1 1 1\n", "silent").unwrap();
    close(csv(&result, "-ac.csv").1[0][1], 0., 0.);
    let large = simulate("large frequency\nV1 in 0 AC 1\nR1 in 0 1k\n.ac lin 1 1e20 1e20\n", "large").unwrap();
    assert!(!large.files.iter().any(|(_, content)| content.contains("NaN")));
}

#[test]
fn noise_matches_thermal_math_differential_output_and_integral() {
    let deck = "thermal noise\nV1 in 0 AC 1\nR1 in out 1k\nR2 out 0 3k\n.noise v(out,in) V1 dec 2 10 10k\n";
    let result = simulate(deck, "thermal").unwrap();
    let (headers, rows) = csv(&result, "-noise.csv");
    assert_eq!(headers, ["freq", "onoise_spectrum", "inoise_spectrum"]);
    let density = (4. * 1.380649e-23 * 300.15 * 750f64).sqrt();
    for row in &rows { close(row[1], density, density * 1e-11); close(row[2], density / 0.25, density * 1e-11); }
    let words: Vec<_> = result.text.split_whitespace().collect();
    let output = words.iter().position(|s| *s == "output").unwrap();
    close(words[output + 2].parse().unwrap(), density * (10000f64 - 10.).sqrt(), density * 1e-8);
    let flat = simulate("one frequency\nV1 in 0 AC 1\nR1 in out 1k\nR2 out 0 1k\n.noise v(out) V1 lin 5 10 10\n", "flat").unwrap();
    assert!(!flat.text.contains("NaN"));
    let current = simulate("current noise gain\nI1 0 out AC 1\nR1 out 0 1k\n.noise v(out) I1 oct 1 1 8\n", "current").unwrap();
    let (_, rows) = csv(&current, "-noise.csv");
    close(rows[0][2], (4. * 1.380649e-23 * 300.15 / 1000f64).sqrt(), 1e-22);
}

#[test]
fn temperature_suffixes_options_model_tnom_and_resistor_tc() {
    let result = simulate("TC\nV1 in 0 1\nR1 in 0 1k TC1=.004 TC2=.00001\n.temp -40 27 85\n.op\n.ac lin 1 1 1\n", "tc").unwrap();
    assert_eq!(result.files.len(), 9);
    for temp in [-40, 27, 85] {
        let table = &result.files.iter().find(|(p, _)| p.ends_with(&format!("-op-{temp}C.txt"))).unwrap().1;
        let current: f64 = table.lines().find(|line| line.starts_with("i(V1)")).unwrap().split_whitespace().nth(2).unwrap().parse().unwrap();
        let delta = temp as f64 - 27.;
        close(current, -1. / (1000. * (1. + 0.004 * delta + 0.00001 * delta * delta)), 1e-15);
    }
    let body = "V1 in 0 1\nR1 in out 1k\nD1 out 0 d\n.model d D(IS=1p N=1.2 EG=1.05 XTI=2 TNOM=40)\n.op\n";
    let options = simulate(&format!("options\n{body}.options temp=85 tnom=10\n"), "t").unwrap();
    let temp = simulate(&format!("options\n{body}.temp 85\n"), "t").unwrap();
    assert_eq!(options.files, temp.files);
    let default = simulate("default\nR1 a 0 1k\n.temp 27\n.op\n", "t").unwrap();
    assert_eq!(default.files[0].0, "sim/t/t-op.txt");
}

#[test]
fn exact_new_errors_and_resource_guards() {
    for (tail, expected) in [
        (".ac bad 1 1 10", "line 4: .ac needs dec, oct or lin"),
        (".ac dec 1 0 10", "line 4: .ac needs fstart > 0 and fstop >= fstart"),
        (".ac lin 1 10 1", "line 4: .ac needs fstart > 0 and fstop >= fstart"),
        (".noise v(out) absent dec 1 1 10", "line 4: .noise needs an output node and an independent source"),
        (".noise v(absent) V1 dec 1 1 10", "line 4: .noise needs an output node and an independent source"),
        (".noise i(out) V1 dec 1 1 10", "line 4: .noise needs an output node and an independent source"),
        (".ac lin 100002 1 10", "line 4: invalid frequency sweep (limit 100001 points)"),
        (".temp -273.15\n.op", "line 4: temperature must exceed absolute zero"),
    ] { assert_eq!(simulate(&format!("bad\nV1 out 0 AC 1\nR1 out 0 1k\n{tail}\n"), "bad").err().unwrap(), expected); }
    for tail in [".ac dec 0 1 10", ".ac oct 1.5 1 10", ".options temp=-273.15\n.op", ".options tnom=-300\n.op", ".temp\n.op"] {
        assert!(simulate(&format!("bad\nR1 out 0 1k\n{tail}\n"), "bad").is_err());
    }
}

#[tokio::test]
async fn tool_writes_temperature_ac_and_noise_files_deterministically() {
    let dir = TempDir::new("ac-noise"); std::fs::create_dir_all(dir.path.join("sim")).unwrap();
    std::fs::write(dir.path.join("sim/test.cir"), "outputs\nV1 in 0 AC 1\nR1 in out 1k\nR2 out 0 1k\n.ac oct 2 1 8\n.noise v(out) V1 lin 3 1 10\n.temp 85\n").unwrap();
    let tool = all_tools().into_iter().find(|t| t.name == "circuit_simulate").unwrap();
    let context = || ToolContext::new(dir.path.clone(), dir.path.clone());
    let first = (tool.run)(json!({"source":"sim/test.cir"}), context()).await.unwrap();
    let bytes: Vec<_> = first.files.iter().map(|p| std::fs::read(dir.path.join(p)).unwrap()).collect();
    let second = (tool.run)(json!({"source":"sim/test.cir"}), context()).await.unwrap();
    assert_eq!(first.text, second.text); assert_eq!(first.files, second.files);
    assert_eq!(first.files, ["sim/test/test-ac-85C.csv", "sim/test/test-ac-85C.svg", "sim/test/test-noise-85C.csv", "sim/test/test-noise-85C.svg"]);
    for (path, expected) in second.files.iter().zip(bytes) { assert_eq!(std::fs::read(dir.path.join(path)).unwrap(), expected); }
}
