mod common;
use common::TempDir;
use forge_server::tools::{circuit::{simulate, Simulation}, exec::{on_path, run}};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

fn fixtures() -> Vec<(String, Value)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sim/ngspice");
    let mut paths: Vec<_> = std::fs::read_dir(root).unwrap().map(|p| p.unwrap().path()).collect();
    paths.sort();
    paths.into_iter().map(|path| {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let f: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(f["ngspice_version"].as_str().unwrap().starts_with("ngspice-"));
        assert_eq!(f["command"], "ngspice -b reference.cir");
        (name, f)
    }).collect()
}
fn deck(f: &Value) -> String {
    let mut control = false;
    f["netlist"].as_str().unwrap().lines().filter(|line| {
        if line.eq_ignore_ascii_case(".control") { control = true; return false; }
        if line.eq_ignore_ascii_case(".endc") { control = false; return false; }
        !control
    }).map(|line| format!("{line}\n")).collect()
}
fn file<'a>(result: &'a Simulation, kind: &str, extension: &str) -> &'a str {
    &result.files.iter().find(|(p, _)| p.contains(&format!("reference-{kind}")) && p.ends_with(extension)).unwrap().1
}
fn csv(result: &Simulation, kind: &str) -> (Vec<String>, Vec<Vec<f64>>) {
    let mut lines = file(result, kind, ".csv").lines();
    let headers = lines.next().unwrap().split(',').map(str::to_owned).collect();
    let rows = lines.map(|line| line.split(',').map(|s| s.parse().unwrap()).collect()).collect();
    (headers, rows)
}
fn interpolate(rows: &[Vec<f64>], time: f64, column: usize) -> f64 {
    let upper = rows.partition_point(|row| row[0] < time);
    if upper == 0 { return rows[0][column]; }
    if upper == rows.len() { return rows.last().unwrap()[column]; }
    let (a, b) = (&rows[upper - 1], &rows[upper]);
    a[column] + (b[column] - a[column]) * (time - a[0]) / (b[0] - a[0])
}
fn crossing(rows: &[Vec<f64>], column: usize, start: f64, rising: bool) -> f64 {
    let pair = rows.windows(2).find(|p| p[0][0] >= start && if rising { p[0][column] <= 2.5 && p[1][column] > 2.5 } else { p[0][column] >= 2.5 && p[1][column] < 2.5 }).unwrap();
    let (a, b) = (&pair[0], &pair[1]);
    a[0] + (b[0] - a[0]) * (2.5 - a[column]) / (b[column] - a[column])
}
fn actual(f: &Value, result: &Simulation) -> Vec<Vec<f64>> {
    let columns: Vec<_> = f["columns"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    let analysis = f["analysis"].as_str().unwrap();
    if analysis == "op" {
        let values: BTreeMap<_, f64> = file(result, "op", ".txt").lines().map(|line| {
            let (label, rest) = line.split_once(" = ").unwrap();
            (label.to_ascii_lowercase(), rest.split_whitespace().next().unwrap().parse().unwrap())
        }).collect();
        return vec![columns.iter().map(|label| values[&label.to_ascii_lowercase()]).collect()];
    }
    let (headers, rows) = csv(result, analysis);
    let indices: Vec<_> = columns.iter().map(|label| headers.iter().position(|h| h.eq_ignore_ascii_case(label)).unwrap()).collect();
    match f["metric"].as_str() {
        Some("peak") => vec![vec![rows.iter().map(|r| r[indices[0]]).fold(f64::NEG_INFINITY, f64::max)]],
        Some("delay") => vec![vec![crossing(&rows, indices[0], 1e-9, false) - 1.5e-9, crossing(&rows, indices[0], 22e-9, true) - 22.5e-9]],
        _ => f["rows"].as_array().unwrap().iter().map(|reference| {
            let axis = reference[0].as_f64().unwrap();
            let mut row = vec![axis];
            row.extend(indices.iter().map(|&k| interpolate(&rows, axis, k))); row
        }).collect(),
    }
}
fn compare(name: &str, f: &Value, rows: &[Vec<f64>], relative: f64, absolute: f64) {
    let references = f["rows"].as_array().unwrap();
    assert_eq!(rows.len(), references.len(), "{name}: point count");
    let has_axis = f["analysis"] != "op" && f["metric"].is_null();
    for (i, (row, reference)) in rows.iter().zip(references).enumerate() {
        assert_eq!(row.len(), reference.as_array().unwrap().len(), "{name}: column count");
        for (k, &value) in row.iter().enumerate().skip(usize::from(has_axis)) {
            let expected = reference[k].as_f64().unwrap();
            let label = if f["metric"] == "delay" { if k == 0 { "fall delay" } else { "rise delay" } }
                else { f["columns"][k - usize::from(has_axis)].as_str().unwrap() };
            let phase = label.starts_with("vp(");
            let error = if phase { ((value - expected + 180.).rem_euclid(360.) - 180.).abs() } else { (value - expected).abs() };
            let scale = f["signal_scale"].as_f64().unwrap_or(expected.abs());
            let limit = if phase { absolute } else { absolute.max(relative * scale) };
            println!("ngspice | {name} | {label}[{i}] | {expected:.12e} | {value:.12e} | {error:.6e}");
            assert!(value.is_finite() && error <= limit.max(1e-18), "{name} {label}[{i}]: ngspice={expected}, Forge={value}, error={error}, limit={limit}");
        }
    }
}
fn compare_totals(name: &str, f: &Value, totals: &[f64]) {
    if let Some(reference) = f["totals"].as_array() {
        for (i, value) in totals.iter().enumerate() {
            let expected = reference[i].as_f64().unwrap();
            let error = (value - expected).abs();
            println!("ngspice | {name} | {} total | {expected:.12e} | {value:.12e} | {error:.6e}", if i == 0 { "output" } else { "input" });
            assert!(error <= expected.abs() * f["relative_tolerance"].as_f64().unwrap(), "{name}: integrated noise {i} error {error}");
        }
    }
}

#[test]
fn offline_ngspice_accuracy() {
    let mut failures = vec![];
    for (name, f) in fixtures() {
        if std::panic::catch_unwind(|| {
            let result = simulate(&deck(&f), "reference").unwrap_or_else(|e| panic!("{name}: {e}"));
            compare(&name, &f, &actual(&f, &result), f["relative_tolerance"].as_f64().unwrap(), f["absolute_tolerance"].as_f64().unwrap());
            if f["analysis"] == "noise" {
                let words: Vec<_> = result.text.split_whitespace().collect();
                let out = words.iter().position(|s| *s == "output").unwrap();
                let input = words.iter().position(|s| *s == "input").unwrap();
                compare_totals(&name, &f, &[words[out + 2].parse().unwrap(), words[input + 2].parse().unwrap()]);
            }
        }).is_err() { failures.push(name); }
    }
    assert!(failures.is_empty(), "failed fixtures: {}", failures.join(", "));
}

#[tokio::test]
async fn optional_live_ngspice_replays_every_fixture() {
    if !tokio::task::spawn_blocking(|| on_path("ngspice")).await.unwrap() {
        eprintln!("skipped: ngspice not installed"); return;
    }
    for (name, f) in fixtures() {
        let dir = TempDir::new("ngspice");
        std::fs::write(dir.path.join("reference.cir"), f["netlist"].as_str().unwrap()).unwrap();
        let result = run("ngspice", &["-b".into(), "reference.cir".into()], &dir.path, 30_000, &[]).await;
        assert_eq!(result.code, 0, "{name}: {} {}", result.stdout, result.stderr);
        let text = std::fs::read_to_string(dir.path.join("reference.dat")).unwrap();
        let rows: Vec<Vec<f64>> = text.lines().skip(1).map(|line| line.split_whitespace().map(|s| s.parse().unwrap()).collect()).collect();
        let rows = if f["analysis"] == "op" { vec![rows[0][1..].to_vec()] }
            else if f["metric"] == "peak" { vec![vec![rows.iter().map(|r| r[1]).fold(f64::NEG_INFINITY, f64::max)]] }
            else if f["metric"] == "delay" { vec![vec![crossing(&rows, 1, 1e-9, false) - 1.5e-9, crossing(&rows, 1, 22e-9, true) - 22.5e-9]] }
            else { f["rows"].as_array().unwrap().iter().map(|r| {
                let axis = r[0].as_f64().unwrap();
                let mut values = vec![axis]; values.extend((1..rows[0].len()).map(|k| interpolate(&rows, axis, k))); values
            }).collect() };
        compare(&format!("live {name}"), &f, &rows, f["relative_tolerance"].as_f64().unwrap().max(1e-6), f["absolute_tolerance"].as_f64().unwrap().max(1e-12));
        if f["analysis"] == "noise" {
            let text = std::fs::read_to_string(dir.path.join("reference_total.dat")).unwrap();
            let totals: Vec<_> = text.lines().nth(1).unwrap().split_whitespace().skip(1).map(|s| s.parse().unwrap()).collect();
            compare_totals(&format!("live {name}"), &f, &totals);
        }
    }
}
