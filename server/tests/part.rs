mod common;
#[path = "common/tools.rs"]
mod support;

use serde_json::{json, Value};
use support::{project, run};

/// Builds a part in a fresh project and returns (project dir, the models/<name>.json contents).
async fn build(input: Value) -> (common::TempDir, Value) {
    let dir = project("part", &[] as &[(&str, &str)]);
    let name = input["name"].as_str().unwrap().to_owned();
    run("part_build", input, &dir.path).await.unwrap();
    let text = std::fs::read_to_string(dir.path.join(format!("models/{name}.json"))).unwrap();
    (dir, serde_json::from_str(&text).unwrap())
}

/// Volume of body number i in the build output.
fn volume(out: &Value, i: usize) -> f64 {
    out["bodies"][i]["volume"].as_f64().unwrap()
}

/// Asserts a is within tol of b.
fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

#[tokio::test]
async fn box_volume() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"box","size":[10,10,10]}]})).await;
    near(volume(&out, 0), 1000.0, 0.01);
}

#[tokio::test]
async fn cylinder_volume() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"cylinder","radius":5,"height":10}]})).await;
    near(volume(&out, 0), 785.398, 0.01);
}

#[tokio::test]
async fn sphere_volume() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"sphere","radius":5}]})).await;
    near(volume(&out, 0), 523.599, 0.01);
}

#[tokio::test]
async fn cone_volume() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"cone","radius1":5,"radius2":0,"height":10}]})).await;
    near(volume(&out, 0), 261.799, 0.01);
}

#[tokio::test]
async fn extrude_rectangle() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"extrude","height":5,"outline":{"start":[0,0],"segments":[{"line":[20,0]},{"line":[20,10]},{"line":[0,10]},{"line":[0,0]}]}}]})).await;
    near(volume(&out, 0), 1000.0, 0.01);
}

#[tokio::test]
async fn extrude_circle() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"extrude","height":10,"outline":{"circle":[0,0,5]}}]})).await;
    near(volume(&out, 0), 785.398, 0.01);
}

#[tokio::test]
async fn revolve_full() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"revolve","outline":{"start":[0,0],"segments":[{"line":[5,0]},{"line":[5,10]},{"line":[0,10]},{"line":[0,0]}]}}]})).await;
    near(volume(&out, 0), 785.398, 0.01);
}

#[tokio::test]
async fn revolve_half() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"revolve","angle":180,"outline":{"start":[0,0],"segments":[{"line":[5,0]},{"line":[5,10]},{"line":[0,10]},{"line":[0,0]}]}}]})).await;
    near(volume(&out, 0), 392.699, 0.01);
}

#[tokio::test]
async fn revolve_ring() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"revolve","outline":{"start":[3,0],"segments":[{"line":[5,0]},{"line":[5,10]},{"line":[3,10]},{"line":[3,0]}]}}]})).await;
    near(volume(&out, 0), 502.655, 0.01);
}

#[tokio::test]
async fn cut_cylinder_from_cube() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[20,20,20]},{"id":"b","op":"cylinder","radius":5,"height":20,"at":[10,10,0]},{"op":"cut","target":"a","tool":"b"}]})).await;
    near(volume(&out, 0), 6429.204, 0.01);
}

#[tokio::test]
async fn union_overlapping_cubes() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[10,10,10]},{"id":"b","op":"box","size":[10,10,10],"at":[5,0,0]},{"op":"union","target":"a","tool":"b"}]})).await;
    near(volume(&out, 0), 1500.0, 0.01);
}

#[tokio::test]
async fn intersect_overlapping_cubes() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[10,10,10]},{"id":"b","op":"box","size":[10,10,10],"at":[5,0,0]},{"op":"intersect","target":"a","tool":"b"}]})).await;
    near(volume(&out, 0), 500.0, 0.01);
}

#[tokio::test]
async fn fillet_vertical_edges() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[20,20,20]},{"op":"fillet","target":"a","radius":2,"edges":"vertical"}]})).await;
    near(volume(&out, 0), 7931.327, 0.01);
}

#[tokio::test]
async fn fillet_all_edges() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[20,20,20]},{"op":"fillet","target":"a","radius":2,"edges":"all"}]})).await;
    near(volume(&out, 0), 7804.696, 0.05);
}

#[tokio::test]
async fn chamfer_vertical_edges() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[20,20,20]},{"op":"chamfer","target":"a","size":2,"edges":"vertical"}]})).await;
    near(volume(&out, 0), 7840.0, 0.01);
}

#[tokio::test]
async fn m3_clearance_hole() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[20,20,5]},{"op":"hole","target":"a","points":[[10,10]],"size":"M3","kind":"clearance"}]})).await;
    near(volume(&out, 0), 1954.604, 0.01);
}

#[tokio::test]
async fn m3_tap_blind_hole() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[20,20,5]},{"op":"hole","target":"a","points":[[10,10]],"size":"M3","kind":"tap","depth":4}]})).await;
    near(volume(&out, 0), 1980.365, 0.01);
}

#[tokio::test]
async fn enclosure_volume() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"enclosure","size":[60,40,20]}]})).await;
    near(volume(&out, 0), 11712.0, 0.01);
}

#[tokio::test]
async fn lid_volume() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"op":"lid","size":[60,40]}]})).await;
    near(volume(&out, 0), 8758.72, 0.01);
}

#[tokio::test]
async fn standoffs_add_volume() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"e","op":"enclosure","size":[60,40,20]},{"op":"standoffs","target":"e","points":[[10,10],[50,30]],"size":"M3","height":5,"base":2,"kind":"insert"}]})).await;
    near(volume(&out, 0), 12088.991, 0.05);
}

#[tokio::test]
async fn lid_fits_with_no_overlap() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"e","op":"enclosure","size":[60,40,20]},{"id":"l","op":"lid","size":[60,40],"at":[0,0,20]},{"op":"intersect","target":"e","tool":"l"}]})).await;
    near(volume(&out, 0), 0.0, 0.001);
}

#[tokio::test]
async fn lid_moved_past_gap_overlaps() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"e","op":"enclosure","size":[60,40,20]},{"id":"l","op":"lid","size":[60,40],"at":[0.25,0,20]},{"op":"intersect","target":"e","tool":"l"}]})).await;
    assert!(volume(&out, 0) > 0.01);
}

#[tokio::test]
async fn lid_moved_within_gap_still_fits() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"e","op":"enclosure","size":[60,40,20]},{"id":"l","op":"lid","size":[60,40],"at":[0.15,0,20]},{"op":"intersect","target":"e","tool":"l"}]})).await;
    near(volume(&out, 0), 0.0, 0.001);
}

#[tokio::test]
async fn place_rotates_then_moves() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[10,20,5]},{"op":"place","target":"a","rotate":[0,0,90],"move":[100,0,0]}]})).await;
    let bbox = &out["bodies"][0]["bbox"];
    near(bbox[0][0].as_f64().unwrap(), 80.0, 0.001);
    near(bbox[1][0].as_f64().unwrap(), 100.0, 0.001);
    near(bbox[0][1].as_f64().unwrap(), 0.0, 0.001);
    near(bbox[1][1].as_f64().unwrap(), 10.0, 0.001);
    near(bbox[1][2].as_f64().unwrap(), 5.0, 0.001);
}

#[tokio::test]
async fn writes_step_stl_and_json_and_step_reads_back() {
    let (dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[20,20,20]},{"id":"b","op":"cylinder","radius":5,"height":20,"at":[10,10,0]},{"op":"cut","target":"a","tool":"b"}]})).await;
    for file in ["models/t.step", "models/t.stl", "models/t.json"] {
        assert!(dir.path.join(file).is_file(), "{file} missing");
    }
    assert_eq!(out["steps"].as_array().unwrap().len(), 3);
    let report = run("part_inspect", json!({"path":"models/t.step"}), &dir.path).await.unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    near(report["volume"].as_f64().unwrap(), 6429.204, 6.43);
    let mesh = run("part_inspect", json!({"path":"models/t.stl"}), &dir.path).await.unwrap();
    let mesh: Value = serde_json::from_str(&mesh.text).unwrap();
    near(mesh["volume"].as_f64().unwrap(), 6429.204, 130.0);
    assert_eq!(mesh["holes"], "unknown (mesh)");
}

#[tokio::test]
async fn inspect_finds_both_m5_holes_and_mass() {
    let (dir, _out) = build(json!({"name":"bracket","steps":[{"id":"a","op":"box","size":[40,20,5]},{"op":"hole","target":"a","points":[[8,10],[32,10]],"size":"M5","kind":"clearance"}]})).await;
    let report = run("part_inspect", json!({"path":"models/bracket.step","material":"Aluminium 6061"}), &dir.path).await.unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    near(report["volume"].as_f64().unwrap(), 3762.417, 0.01);
    near(report["mass_g"].as_f64().unwrap(), 10.159, 0.001);
    let holes = report["holes"].as_array().unwrap();
    assert_eq!(holes.len(), 2);
    for hole in holes {
        assert_eq!(hole["fits"], "M5 clearance");
        assert_eq!(hole["through"], true);
        near(hole["diameter"].as_f64().unwrap(), 5.5, 0.001);
    }
    assert_eq!(report["overhang_warning"], false);
}

#[tokio::test]
async fn inspect_warns_about_overhang() {
    let (dir, _out) = build(json!({"name":"tee","steps":[{"id":"a","op":"box","size":[10,10,10]},{"id":"b","op":"box","size":[30,10,5],"at":[-10,0,10]},{"op":"union","target":"a","tool":"b"}]})).await;
    let report = run("part_inspect", json!({"path":"models/tee.step"}), &dir.path).await.unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    near(report["overhang_area"].as_f64().unwrap(), 200.0, 0.5);
    assert_eq!(report["overhang_warning"], true);
}

#[tokio::test]
async fn errors_name_the_step() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run("part_build", json!({"name":"t","steps":[{"op":"box","size":[1,1,1]},{"op":"teapot"}]}), &dir.path).await.unwrap_err();
    assert!(err.to_string().contains("step 1: unknown op \"teapot\""), "{err}");
    let err = run("part_build", json!({"name":"t","steps":[{"op":"cut","target":"x","tool":"y"}]}), &dir.path).await.unwrap_err();
    assert!(err.to_string().contains("step 0: no body \"x\""), "{err}");
    let err = run("part_build", json!({"name":"t","steps":[{"id":"a","op":"box","size":[2,2,2]},{"op":"fillet","target":"a","radius":5,"edges":"all"}]}), &dir.path).await.unwrap_err();
    assert!(err.to_string().contains("step 1: fillet failed"), "{err}");
}
