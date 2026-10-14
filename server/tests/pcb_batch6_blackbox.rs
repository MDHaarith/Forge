mod common;
#[path = "common/tools.rs"]
mod support;

use common::TempDir;
use forge_server::{
    http::{router, AppState},
    model::{Model, OfflineModel, SwitchableModel},
    projects::Projects,
    tools::pcb::{self, NativeBuild},
    Settings,
};
use serde_json::{json, Value};
use std::sync::Arc;

struct TestServer {
    _temp: TempDir,
    base: String,
    state: AppState,
    task: tokio::task::JoinHandle<()>,
}

impl TestServer {
    async fn start(model: Arc<dyn Model>) -> Self {
        let temp = TempDir::new("http");
        let settings = Settings::new(temp.path.join("home"), temp.path.join("app"));
        let selected = Arc::new(SwitchableModel::new("offline", model));
        let state = AppState::new(
            Arc::new(Projects::new(settings.clone(), selected.clone())),
            selected,
            settings,
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = router(state.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            _temp: temp,
            base,
            state,
            task,
        }
    }

    async fn offline() -> Self {
        Self::start(Arc::new(OfflineModel)).await
    }

    async fn post(&self, path: &str, body: Value) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("{}/api{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn circuit(built: &NativeBuild) -> Vec<Value> {
    serde_json::from_str(&built.files[0].1).unwrap()
}

fn err_msg(res: Result<NativeBuild, String>) -> String {
    match res {
        Ok(_) => panic!("expected Err, but build succeeded"),
        Err(e) => e,
    }
}

async fn drc(spec: &Value, built: &NativeBuild) -> String {
    let project = support::project(
        "batch6-bb-drc",
        &[
            ("pcb/test.pcb.json", spec.to_string()),
            ("pcb/test.circuit.json", built.files[0].1.clone()),
        ],
    );
    support::run("pcb_drc", json!({"board": "test"}), &project.path)
        .await
        .unwrap()
        .text
}

fn terminals() -> Value {
    json!({
        "board": {"width": 30, "height": 24},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x1", "at": [-8, -3]},
            {"ref": "J2", "kind": "header", "footprint": "pinheader_1x1", "at": [8, 3]}
        ],
        "nets": {"N": ["J1.1", "J2.1"]}
    })
}

/// 1. Locked track kept verbatim: a locked 3-point track for one net appears as a `pcb_trace`
/// with exactly those points (jsfmt numbers) with beautify on and off; DRC clean;
/// other nets do not cross it (DRC proves it).
#[tokio::test]
async fn test1_locked_track_kept_verbatim_beautify_on_off_drc_clean_no_cross() {
    let mut spec = terminals();
    spec["parts"].as_array_mut().unwrap().extend([
        json!({"ref": "J3", "kind": "header", "footprint": "pinheader_1x1", "at": [-2, -8]}),
        json!({"ref": "J4", "kind": "header", "footprint": "pinheader_1x1", "at": [-2, 8]}),
    ]);
    spec["nets"]["CROSS"] = json!(["J3.1", "J4.1"]);
    spec["tracks"] = json!([{
        "net": "N",
        "layer": "top",
        "width": 0.3,
        "points": [[-8, -3], [8, -3], [8, 3]]
    }]);

    for beautify in [true, false] {
        spec["rules"] = json!({"beautify": beautify});
        let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
        let traces: Vec<_> = circuit(&built)
            .into_iter()
            .filter(|e| e["type"] == "pcb_trace" && e["net"] == "N")
            .collect();

        assert_eq!(traces.len(), 2, "locked 3-point track must yield exactly 2 trace segments");
        assert_eq!(
            traces[0]["route"],
            json!([
                {"route_type": "wire", "x": -8, "y": -3, "width": 0.3, "layer": "top"},
                {"route_type": "wire", "x": 8, "y": -3, "width": 0.3, "layer": "top"}
            ])
        );
        assert_eq!(
            traces[1]["route"],
            json!([
                {"route_type": "wire", "x": 8, "y": -3, "width": 0.3, "layer": "top"},
                {"route_type": "wire", "x": 8, "y": 3, "width": 0.3, "layer": "top"}
            ])
        );

        let cross_traces: Vec<_> = circuit(&built)
            .into_iter()
            .filter(|e| e["type"] == "pcb_trace" && e["net"] == "CROSS")
            .collect();
        assert!(!cross_traces.is_empty(), "CROSS net must be routed");

        let report = drc(&spec, &built).await;
        assert!(
            report.ends_with("No DRC errors."),
            "DRC failed with beautify={beautify}: {report}"
        );
    }
}

/// 2. A net fully connected by locked tracks (+ a locked via between layers) gets no extra `pcb_trace` for that net.
#[tokio::test]
async fn test2_net_fully_connected_by_locked_tracks_and_via_gets_no_extra_trace() {
    let mut spec = terminals();
    spec["tracks"] = json!([
        {"net": "N", "layer": "top", "points": [[-8, -3], [0, -3]]},
        {"net": "N", "layer": "bottom", "points": [[0, -3], [8, 3]]}
    ]);
    spec["vias"] = json!([{"net": "N", "at": [0, -3]}]);

    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&built);

    let traces: Vec<_> = elements
        .iter()
        .filter(|e| e["type"] == "pcb_trace" && e["net"] == "N")
        .collect();
    assert_eq!(
        traces.len(),
        2,
        "fully connected net must have only the 2 locked trace segments and no extra traces"
    );

    let vias: Vec<_> = elements
        .iter()
        .filter(|e| e["type"] == "pcb_via" && e["net"] == "N")
        .collect();
    assert_eq!(vias.len(), 1, "must have exactly the 1 locked via");
    assert_eq!(vias[0]["x"], 0);
    assert_eq!(vias[0]["y"], -3);

    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

/// 3. Locked copper appears in the F_Cu/B_Cu Gerbers (more draws than the same spec without the locked track on an otherwise
/// unrouted net) and in the IPC netlist via records if a via is locked.
#[test]
fn test3_locked_copper_in_gerbers_and_ipc_vias() {
    let base_spec = json!({
        "board": {"width": 30, "height": 20, "layers": 2},
        "parts": [
            {"ref": "R1", "kind": "resistor", "footprint": "0402", "at": [-10, 0]},
            {"ref": "R2", "kind": "resistor", "footprint": "0402", "at": [10, 0]}
        ],
        "nets": {"X": []}
    });

    let unrouted = pcb::build(&base_spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let f_cu_unrouted = &unrouted.files.iter().find(|(p, _)| p.ends_with("-F_Cu.gbr")).unwrap().1;
    let b_cu_unrouted = &unrouted.files.iter().find(|(p, _)| p.ends_with("-B_Cu.gbr")).unwrap().1;
    let ipc_unrouted = &unrouted.files.iter().find(|(p, _)| p.ends_with(".ipc")).unwrap().1;

    let f_draws_unrouted = f_cu_unrouted.matches("D01*").count();
    let b_draws_unrouted = b_cu_unrouted.matches("D01*").count();
    assert!(!ipc_unrouted.contains("VIA"), "IPC netlist without vias must not contain VIA");

    let mut locked_spec = base_spec.clone();
    locked_spec["tracks"] = json!([
        {"net": "X", "layer": "top", "points": [[-5, 0], [0, 0]]},
        {"net": "X", "layer": "bottom", "points": [[0, 0], [5, 0]]}
    ]);
    locked_spec["vias"] = json!([{"net": "X", "at": [0, 0]}]);

    let locked = pcb::build(&locked_spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let f_cu_locked = &locked.files.iter().find(|(p, _)| p.ends_with("-F_Cu.gbr")).unwrap().1;
    let b_cu_locked = &locked.files.iter().find(|(p, _)| p.ends_with("-B_Cu.gbr")).unwrap().1;
    let ipc_locked = &locked.files.iter().find(|(p, _)| p.ends_with(".ipc")).unwrap().1;

    let f_draws_locked = f_cu_locked.matches("D01*").count();
    let b_draws_locked = b_cu_locked.matches("D01*").count();

    assert!(
        f_draws_locked > f_draws_unrouted,
        "F_Cu must have more draws with locked track ({f_draws_locked} vs {f_draws_unrouted})"
    );
    assert!(
        b_draws_locked > b_draws_unrouted,
        "B_Cu must have more draws with locked track ({b_draws_locked} vs {b_draws_unrouted})"
    );
    assert!(
        ipc_locked.contains("VIA"),
        "IPC netlist must contain VIA record when via is locked"
    );
}

/// 4. Every validation error text in section 1 and section 4 of the brief, one assertion each.
#[tokio::test]
async fn test4_validation_errors_section_1_and_section_4() {
    // Section 1 validation errors
    // 1. tracks[{i}]: unknown net {net}
    let mut spec = terminals();
    spec["tracks"] = json!([{"net": "BAD", "layer": "top", "points": [[0, 0], [1, 0]]}]);
    assert_eq!(
        err_msg(pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json")),
        "tracks[0]: unknown net BAD"
    );

    // 2. tracks[{i}]: needs at least 2 points
    let mut spec = terminals();
    spec["tracks"] = json!([{"net": "N", "layer": "top", "points": [[0, 0]]}]);
    assert_eq!(
        err_msg(pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json")),
        "tracks[0]: needs at least 2 points"
    );

    // 3. tracks[{i}]: layer {layer} needs 4 layers
    let mut spec = terminals();
    spec["tracks"] = json!([{"net": "N", "layer": "inner1", "points": [[0, 0], [1, 0]]}]);
    assert_eq!(
        err_msg(pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json")),
        "tracks[0]: layer inner1 needs 4 layers"
    );

    // 4. vias[{i}]: unknown net {net}
    let mut spec = terminals();
    spec["vias"] = json!([{"net": "BAD", "at": [0, 0]}]);
    assert_eq!(
        err_msg(pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json")),
        "vias[0]: unknown net BAD"
    );

    // 5. vias[{i}]: from and to must differ
    let mut spec = terminals();
    spec["vias"] = json!([{"net": "N", "at": [0, 0], "from": "top", "to": "top"}]);
    assert_eq!(
        err_msg(pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json")),
        "vias[0]: from and to must differ"
    );

    // 6. tracks[{i}]: point {j} is outside the board
    let mut spec = terminals();
    spec["tracks"] = json!([{"net": "N", "layer": "top", "points": [[0, 0], [20, 0]]}]);
    assert_eq!(
        err_msg(pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json")),
        "tracks[0]: point 1 is outside the board"
    );

    // 7. Width via positive helper -> tracks[{i}].width must be positive
    let mut spec = terminals();
    spec["tracks"] = json!([{"net": "N", "layer": "top", "width": 0, "points": [[0, 0], [1, 0]]}]);
    assert_eq!(
        err_msg(pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json")),
        "tracks[0].width must be positive"
    );

    // Section 4 validation errors
    // 8. footprints.{name}: clashes with a built-in footprint
    let mut spec = terminals();
    spec["footprints"] = json!({
        "0603": {
            "pads": [{"number": "1", "at": [0, 0], "size": [1, 1], "shape": "rect", "drill": null, "layers": "top", "rotation": 0}],
            "height": 1
        }
    });
    assert_eq!(
        err_msg(pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json")),
        "footprints.0603: clashes with a built-in footprint"
    );

    // 9. kicad: not a footprint file
    let project = support::project(
        "bb_err_kicad1",
        &[
            ("pcb/test.pcb.json", terminals().to_string()),
            ("parts/bad.kicad_mod", "(kicad_pcb (version 1))".to_string()),
        ],
    );
    let err = support::run(
        "pcb_import_footprint",
        json!({"file": "parts/bad.kicad_mod", "board": "pcb/test.pcb.json"}),
        &project.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert_eq!(err, "kicad: not a footprint file");

    // 10. kicad: pad {n} has no size
    let project = support::project(
        "bb_err_kicad2",
        &[
            ("pcb/test.pcb.json", terminals().to_string()),
            ("parts/nosize.kicad_mod", "(footprint X (pad 1 smd rect (at 0 0)))".to_string()),
        ],
    );
    let err = support::run(
        "pcb_import_footprint",
        json!({"file": "parts/nosize.kicad_mod", "board": "pcb/test.pcb.json"}),
        &project.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert_eq!(err, "kicad: pad 1 has no size");

    // 11. kicad: unbalanced parentheses
    let project = support::project(
        "bb_err_kicad3",
        &[
            ("pcb/test.pcb.json", terminals().to_string()),
            ("parts/unbalanced.kicad_mod", "(footprint X".to_string()),
        ],
    );
    let err = support::run(
        "pcb_import_footprint",
        json!({"file": "parts/unbalanced.kicad_mod", "board": "pcb/test.pcb.json"}),
        &project.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert_eq!(err, "kicad: unbalanced parentheses");
}

/// 5. HTTP: move a part (new `at` rounded to 0.01, rotation normalized e.g. 450 -> 90) is written to the spec on disk with all other
/// keys in their original order; response has `report`. Lock a routed net -> spec gains `tracks` for it; unlock removes them;
/// moving a part on a locked net returns that net in `unlocked`. Unknown part -> 404 `no part X`. Source not ending `.pcb.json`
/// -> error text equal to `pcb_build`'s. Lock before build -> `board {name} is not built; run pcb_build first`.
#[tokio::test]
async fn test5_http_move_lock_unlock_and_errors() {
    let server = TestServer::offline().await;
    server.state.projects.create("board_proj").await.unwrap();
    let dir = server.state.projects.dir("board_proj").unwrap();
    std::fs::create_dir_all(dir.join("pcb")).unwrap();
    let source = dir.join("pcb/main.pcb.json");
    let original = json!({
        "board": {"width": 30, "height": 24},
        "rules": {"trace": 0.25},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x1", "at": [-7, 0]},
            {"ref": "J2", "kind": "header", "footprint": "pinheader_1x1", "at": [7, 0]}
        ],
        "nets": {"N": ["J1.1", "J2.1"]}
    });
    std::fs::write(&source, original.to_string()).unwrap();
    let path = "/projects/board_proj/board";

    // Lock before build -> 422 "board {name} is not built; run pcb_build first"
    let response = server.post(path, json!({"source": "pcb/main.pcb.json", "lock": "N"})).await;
    assert_eq!(response.status(), 422);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"],
        "board main is not built; run pcb_build first"
    );

    // Move part: new `at` rounded to 0.01, rotation normalized e.g. 450 -> 90
    let response = server.post(
        path,
        json!({
            "source": "pcb/main.pcb.json",
            "move": {"ref": "J1", "at": [-6.1234, 1.2367], "rotation": 450, "side": "bottom"}
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    let result: Value = response.json().await.unwrap();
    assert!(result["report"].as_str().unwrap().contains("No errors: every connection is routed."));
    assert_eq!(result["unlocked"], json!([]));

    // Verify written spec on disk: rounding, rotation, and original key order preserved
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&source).unwrap()).unwrap();
    assert_eq!(saved["parts"][0]["at"], json!([-6.12, 1.24]));
    assert_eq!(saved["parts"][0]["rotation"], 90);
    assert_eq!(saved["parts"][0]["side"], "bottom");
    let keys: Vec<&str> = saved.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["board", "rules", "parts", "nets"]);

    // Lock a routed net -> spec gains `tracks` for it
    let response = server.post(path, json!({"source": "pcb/main.pcb.json", "lock": "N"})).await;
    assert_eq!(response.status(), 200);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&source).unwrap()).unwrap();
    assert!(!saved["tracks"].as_array().unwrap().is_empty());

    // Unlock removes them
    let response = server.post(path, json!({"source": "pcb/main.pcb.json", "unlock": "N"})).await;
    assert_eq!(response.status(), 200);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&source).unwrap()).unwrap();
    assert!(saved["tracks"].as_array().unwrap().is_empty());

    // Relock N, then moving a part on net N returns N in `unlocked`
    let response = server.post(path, json!({"source": "pcb/main.pcb.json", "lock": "N"})).await;
    assert_eq!(response.status(), 200);
    let response = server.post(
        path,
        json!({
            "source": "pcb/main.pcb.json",
            "move": {"ref": "J2", "at": [6, 0]}
        }),
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap()["unlocked"], json!(["N"]));

    // Unknown part -> 404 "no part X"
    let response = server.post(
        path,
        json!({
            "source": "pcb/main.pcb.json",
            "move": {"ref": "MISSING", "at": [0, 0]}
        }),
    )
    .await;
    assert_eq!(response.status(), 404);
    assert_eq!(response.json::<Value>().await.unwrap()["error"], "no part MISSING");

    // Source not ending `.pcb.json` -> error text equal to `pcb_build`'s
    let project = support::project("bb_err_cmp", &[("pcb/main.json", "{}")]);
    let pcb_build_err = support::run("pcb_build", json!({"source": "pcb/main.json"}), &project.path)
        .await
        .unwrap_err()
        .to_string();
    let response = server.post(path, json!({"source": "pcb/main.json", "unlock": "N"})).await;
    assert_eq!(response.status(), 400);
    let http_err = response.json::<Value>().await.unwrap()["error"].as_str().unwrap().to_string();
    assert_eq!(http_err, pcb_build_err);
}

/// 6. KiCad import: a hand-written SOT-23 style `.kicad_mod` (KiCad 6+ `(footprint ...)` syntax, y-down) imported through
/// `pcb_import_footprint` -> spec gets `footprints.<name>`; pad y coordinates are negated vs the file; sizes and rotation kept;
/// a part using it builds and passes DRC. Legacy `(module ...)` syntax also parses. `thru_hole oval` with `(drill oval 1 2)`
/// appears in the PTH drill file as a slot (`G85`). Garbage text -> `kicad: not a footprint file`; unbalanced -> `kicad:
/// unbalanced parentheses`; footprint named like a built-in (e.g. `0603`) used in spec -> clash error.
#[tokio::test]
async fn test6_kicad_import_footprint_and_drill_slot_and_errors() {
    let sot23_mod = r#"(footprint "SOT-23-Custom"
  (fp_rect (start -2 -2) (end 2 2) (layer "F.CrtYd"))
  (fp_poly (pts (xy -0.8 -1.2) (xy 0.8 -1.2) (xy 0.8 1.2) (xy -0.8 1.2)) (layer "F.Fab"))
  (pad "1" smd rect (at -0.95 1.0) (size 0.6 0.8) (layers "F.Cu" "F.Paste"))
  (pad "2" smd rect (at 0.95 1.0) (size 0.6 0.8) (layers "F.Cu" "F.Paste"))
  (pad "3" smd rect (at 0.0 -1.0 90) (size 0.6 0.8) (layers "F.Cu" "F.Paste"))
)"#;

    let base_board = json!({
        "board": {"width": 20, "height": 16},
        "parts": []
    });

    let project = support::project(
        "bb_kicad_import",
        &[
            ("pcb/main.pcb.json", base_board.to_string()),
            ("parts/sot23.kicad_mod", sot23_mod.to_string()),
        ],
    );

    // Import SOT-23 footprint
    let result = support::run(
        "pcb_import_footprint",
        json!({"file": "parts/sot23.kicad_mod", "board": "pcb/main.pcb.json"}),
        &project.path,
    )
    .await
    .unwrap();
    assert!(result.text.contains("3 pad(s)"), "{}", result.text);

    // Read spec on disk: check footprints, negated y, sizes, rotation
    let spec_text = std::fs::read_to_string(project.path.join("pcb/main.pcb.json")).unwrap();
    let mut spec: Value = serde_json::from_str(&spec_text).unwrap();
    let fp = &spec["footprints"]["SOT-23-Custom"];
    assert!(fp.is_object(), "spec must contain footprints.SOT-23-Custom");
    // Pad 1 at y = 1.0 in file -> -1 in spec
    assert_eq!(fp["pads"][0]["at"], json!([-0.95, -1]));
    assert_eq!(fp["pads"][0]["size"], json!([0.6, 0.8]));
    // Pad 2 at y = 1.0 in file -> -1 in spec
    assert_eq!(fp["pads"][1]["at"], json!([0.95, -1]));
    // Pad 3 at y = -1.0 in file -> 1 in spec, rotation 90
    assert_eq!(fp["pads"][2]["at"], json!([0, 1]));
    assert_eq!(fp["pads"][2]["rotation"], 90);
    assert_eq!(fp["pads"][2]["size"], json!([0.6, 0.8]));

    // A part using it builds and passes DRC
    spec["parts"] = json!([
        {"ref": "U1", "kind": "chip", "footprint": "SOT-23-Custom", "at": [0, 0]}
    ]);
    std::fs::write(project.path.join("pcb/main.pcb.json"), spec.to_string()).unwrap();
    let build_res = support::run(
        "pcb_build",
        json!({"source": "pcb/main.pcb.json"}),
        &project.path,
    )
    .await
    .unwrap();
    assert!(build_res.text.contains("Built") && build_res.text.contains("No errors"), "{}", build_res.text);

    let drc_res = support::run("pcb_drc", json!({"board": "main"}), &project.path)
        .await
        .unwrap();
    assert!(drc_res.text.ends_with("No DRC errors."), "{}", drc_res.text);

    // Legacy (module ...) syntax also parses
    let legacy_mod = r#"(module LegacyModule
  (fp_line (start -2 -2) (end 2 2) (layer F.CrtYd))
  (pad 1 smd rect (at 0 0) (size 1 1) (layers F.Cu))
)"#;
    std::fs::write(project.path.join("parts/legacy.kicad_mod"), legacy_mod).unwrap();
    let result = support::run(
        "pcb_import_footprint",
        json!({"file": "parts/legacy.kicad_mod", "board": "pcb/main.pcb.json"}),
        &project.path,
    )
    .await
    .unwrap();
    assert!(result.text.contains("1 pad(s)"), "{}", result.text);
    let spec_text = std::fs::read_to_string(project.path.join("pcb/main.pcb.json")).unwrap();
    let spec: Value = serde_json::from_str(&spec_text).unwrap();
    assert!(spec["footprints"]["LegacyModule"].is_object());

    // thru_hole oval with (drill oval 1 2) appears in the PTH drill file as a slot (G85)
    let slot_mod = r#"(footprint "SlotModule"
  (pad 1 thru_hole oval (at 0 0) (size 2 3) (drill oval 1 2) (layers *.Cu))
)"#;
    std::fs::write(project.path.join("parts/slot.kicad_mod"), slot_mod).unwrap();
    support::run(
        "pcb_import_footprint",
        json!({"file": "parts/slot.kicad_mod", "board": "pcb/main.pcb.json"}),
        &project.path,
    )
    .await
    .unwrap();
    let mut spec: Value = serde_json::from_str(&std::fs::read_to_string(project.path.join("pcb/main.pcb.json")).unwrap()).unwrap();
    spec["parts"] = json!([
        {"ref": "J1", "kind": "header", "footprint": "SlotModule", "at": [0, 0]}
    ]);
    std::fs::write(project.path.join("pcb/main.pcb.json"), spec.to_string()).unwrap();
    support::run("pcb_build", json!({"source": "pcb/main.pcb.json"}), &project.path).await.unwrap();
    let pth_drl = std::fs::read_to_string(project.path.join("fab/main/main-PTH.drl")).unwrap();
    assert!(pth_drl.contains("G85"), "PTH drill file must contain G85 slot command");

    // Garbage text -> "kicad: not a footprint file"
    std::fs::write(project.path.join("parts/garbage.kicad_mod"), "invalid kicad footprint").unwrap();
    let err = support::run(
        "pcb_import_footprint",
        json!({"file": "parts/garbage.kicad_mod", "board": "pcb/main.pcb.json"}),
        &project.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert_eq!(err, "kicad: not a footprint file");

    // Unbalanced parentheses -> "kicad: unbalanced parentheses"
    std::fs::write(project.path.join("parts/unbalanced.kicad_mod"), "(footprint Unbalanced (pad 1 smd rect").unwrap();
    let err = support::run(
        "pcb_import_footprint",
        json!({"file": "parts/unbalanced.kicad_mod", "board": "pcb/main.pcb.json"}),
        &project.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert_eq!(err, "kicad: unbalanced parentheses");

    // Footprint named like a built-in (e.g. 0603) used in spec -> clash error
    let err = support::run(
        "pcb_import_footprint",
        json!({"file": "parts/sot23.kicad_mod", "board": "pcb/main.pcb.json", "name": "0603"}),
        &project.path,
    )
    .await
    .unwrap_err()
    .to_string();
    assert_eq!(err, "footprints.0603: clashes with a built-in footprint");
}

/// 7. Defaults: a spec without `tracks`/`vias`/`footprints` builds identically to one with empty `[]`/`[]`/`{}`.
#[test]
fn test7_defaults_empty_matches_omitted() {
    let spec_omitted = json!({
        "board": {"width": 30, "height": 20, "layers": 2},
        "parts": [
            {"ref": "R1", "kind": "resistor", "footprint": "0402", "at": [-5, 0]},
            {"ref": "R2", "kind": "resistor", "footprint": "0402", "at": [5, 0]}
        ],
        "nets": {"N": ["R1.1", "R2.1"]}
    });

    let mut spec_empty = spec_omitted.clone();
    spec_empty["tracks"] = json!([]);
    spec_empty["vias"] = json!([]);
    spec_empty["footprints"] = json!({});

    let built_omitted = pcb::build(&spec_omitted.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let built_empty = pcb::build(&spec_empty.to_string(), "test", "pcb/test.pcb.json").unwrap();

    assert_eq!(built_omitted.report, built_empty.report);
    assert_eq!(built_omitted.files, built_empty.files);
    assert_eq!(built_omitted.binary_files, built_empty.binary_files);
}

/// 8. Determinism: build with locked copper twice -> identical.
#[test]
fn test8_determinism_locked_copper() {
    let spec = json!({
        "board": {"width": 30, "height": 20, "layers": 4},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x1", "at": [-8, -3]},
            {"ref": "J2", "kind": "header", "footprint": "pinheader_1x1", "at": [8, 3]}
        ],
        "nets": {"N": ["J1.1", "J2.1"]},
        "tracks": [
            {"net": "N", "layer": "top", "points": [[-8, -3], [0, -3]]},
            {"net": "N", "layer": "bottom", "points": [[0, -3], [8, 3]]}
        ],
        "vias": [
            {"net": "N", "at": [0, -3]}
        ]
    });

    let built1 = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let built2 = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();

    assert_eq!(built1.report, built2.report);
    assert_eq!(built1.files, built2.files);
    assert_eq!(built1.binary_files, built2.binary_files);
}
