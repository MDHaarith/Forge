mod common;
#[path = "common/tools.rs"]
mod support;

use forge_server::tools::pcb;
use serde_json::{json, Value};

fn circuit(built: &pcb::NativeBuild) -> Vec<Value> {
    serde_json::from_str(&built.files[0].1).unwrap()
}
async fn drc(spec: &Value, built: &pcb::NativeBuild) -> String {
    let project = support::project("batch6-drc", &[
        ("pcb/test.pcb.json", spec.to_string()), ("pcb/test.circuit.json", built.files[0].1.clone())]);
    support::run("pcb_drc", json!({"board":"test"}), &project.path).await.unwrap().text
}
fn terminals() -> Value {
    json!({"board":{"width":30,"height":24},"parts":[
        {"ref":"J1","kind":"header","footprint":"pinheader_1x1","at":[-8,-3]},
        {"ref":"J2","kind":"header","footprint":"pinheader_1x1","at":[8,3]}],
        "nets":{"N":["J1.1","J2.1"]}})
}

#[tokio::test]
async fn locked_l_shape_survives_beautification_and_other_nets_avoid_it() {
    let mut spec = terminals();
    spec["parts"].as_array_mut().unwrap().extend([
        json!({"ref":"J3","kind":"header","footprint":"pinheader_1x1","at":[-2,-8]}),
        json!({"ref":"J4","kind":"header","footprint":"pinheader_1x1","at":[-2,8]})]);
    spec["nets"]["CROSS"] = json!(["J3.1","J4.1"]);
    spec["tracks"] = json!([{"net":"N","layer":"top","width":0.3,"points":[[-8,-3],[8,-3],[8,3]]}]);
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let copper: Vec<_> = circuit(&built).into_iter().filter(|e| e["type"] == "pcb_trace" && e["net"] == "N").collect();
    assert_eq!(copper.len(), 2, "fully connected locked net must get no new tracks");
    for (trace, pair) in copper.iter().zip([[[-8,-3],[8,-3]], [[8,-3],[8,3]]]) {
        assert_eq!(trace["route"], json!(pair.map(|p| json!({"route_type":"wire","x":p[0],"y":p[1],"width":0.3,"layer":"top"}))));
        assert!(trace.get("is_antenna_trace").is_none());
        assert!(trace.get("pcb_component_id").is_none());
    }
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
    let again = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    assert_eq!(built.files, again.files);
    assert_eq!(built.binary_files, again.binary_files);
}

#[tokio::test]
async fn locked_tracks_and_via_are_real_copper_and_need_no_new_routing() {
    let mut spec = terminals();
    let original = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    spec["tracks"] = json!([
        {"net":"N","layer":"top","points":[[-8,-3],[0,-3]]},
        {"net":"N","layer":"bottom","points":[[0,-3],[8,3]]}]);
    spec["vias"] = json!([{"net":"N","at":[0,-3]}]);
    spec["rules"] = json!({"teardrops":true});
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&built);
    assert_eq!(elements.iter().filter(|e| e["type"] == "pcb_trace").count(), 2);
    let vias: Vec<_> = elements.iter().filter(|e| e["type"] == "pcb_via").collect();
    assert_eq!(vias.len(), 1);
    assert_eq!(vias[0]["layers"], json!(["top","bottom"]));
    assert_eq!(vias[0]["x"], 0);
    assert_eq!(vias[0]["y"], -3);
    assert!(!elements.iter().any(|e| e["type"] == "pcb_trace_teardrop"));
    let ipc = &built.files.iter().find(|(p,_)| p.ends_with(".ipc")).unwrap().1;
    assert!(ipc.contains("VIA"));
    assert!(built.binary_files[0].1.len() > original.binary_files[0].1.len(), "locked copper must add visible 3D geometry");
    assert_ne!(built.files.iter().find(|(p,_)| p.ends_with(".step")).unwrap().1,
        original.files.iter().find(|(p,_)| p.ends_with(".step")).unwrap().1);
    assert!(built.files.iter().find(|(p,_)| p.ends_with("-PTH.drl")).unwrap().1.contains("X0.0Y-3.0"));
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[tokio::test]
async fn length_matching_counts_locked_copper_without_changing_it() {
    let mut spec = json!({"board":{"width":30,"height":24},"parts":[
        {"ref":"J1","kind":"header","footprint":"pinheader_1x1","at":[-8,3]},
        {"ref":"J2","kind":"header","footprint":"pinheader_1x1","at":[8,3]},
        {"ref":"J3","kind":"header","footprint":"pinheader_1x1","at":[-8,-3]},
        {"ref":"J4","kind":"header","footprint":"pinheader_1x1","at":[8,-3]}],
        "nets":{"LONG":["J1.1","J2.1"],"SHORT":["J3.1","J4.1"]},
        "length_groups":{"BUS":{"nets":["LONG","SHORT"],"tolerance":0.5}}});
    spec["tracks"] = json!([{"net":"LONG","layer":"top","points":[[-8,3],[-8,7],[8,7],[8,3]]}]);
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let long: Vec<_> = circuit(&built).into_iter().filter(|e| e["type"] == "pcb_trace" && e["net"] == "LONG").collect();
    assert_eq!(long.len(), 3);
    assert!(built.report.contains("24.00 mm"), "{}", built.report);
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[tokio::test]
async fn partial_locked_copper_routes_only_missing_connections_and_islands() {
    let mut spec = terminals();
    spec["tracks"] = json!([
        {"net":"N","layer":"top","points":[[-8,-3],[-3,-3]]},
        {"net":"N","layer":"top","points":[[2,0],[4,0]]}]);
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&built);
    assert!(elements.iter().filter(|e| e["type"] == "pcb_trace").count() > 2);
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[tokio::test]
async fn locked_blind_via_connects_to_an_inner_reference_plane() {
    let x = pcb::footprint("0603").unwrap().pads[1].at[0];
    let spec = json!({"board":{"width":24,"height":16,"layers":4},"rules":{"blind_vias":true},"parts":[
        {"ref":"R1","kind":"resistor","footprint":"0603","at":[-7,0]},
        {"ref":"R2","kind":"resistor","footprint":"0603","at":[7,0]}],
        "nets":{"GND":["R1.2","R2.1"]},"pours":[{"net":"GND","layer":"inner1"}],
        "tracks":[{"net":"GND","layer":"top","points":[[-7.0+x,0],[-3,0]]}],
        "vias":[{"net":"GND","at":[-3,0],"from":"top","to":"inner1"}]});
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&built);
    let vias: Vec<_> = elements.iter().filter(|e| e["type"] == "pcb_via").collect();
    assert_eq!(vias.len(), 2, "one locked via plus one missing plane connection: {vias:?}");
    assert!(vias.iter().any(|v| v["x"] == -3 && v["layers"] == json!(["top","inner1"])));
    assert!(built.files.iter().any(|(p,_)| p.ends_with("-PTH-1-2.drl")));
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[test]
fn locked_copper_errors_are_exact() {
    for (field, entry, error) in [
        ("tracks", json!({"net":"BAD","layer":"top","points":[[0,0],[1,0]]}), "tracks[0]: unknown net BAD"),
        ("tracks", json!({"net":"N","layer":"top","points":[[0,0]]}), "tracks[0]: needs at least 2 points"),
        ("tracks", json!({"net":"N","layer":"inner1","points":[[0,0],[1,0]]}), "tracks[0]: layer inner1 needs 4 layers"),
        ("tracks", json!({"net":"N","layer":"top","points":[[0,0],[20,0]]}), "tracks[0]: point 1 is outside the board"),
        ("tracks", json!({"net":"N","layer":"top","width":0,"points":[[0,0],[1,0]]}), "tracks[0].width must be positive"),
        ("vias", json!({"net":"BAD","at":[0,0]}), "vias[0]: unknown net BAD"),
        ("vias", json!({"net":"N","at":[0,0],"from":"top","to":"top"}), "vias[0]: from and to must differ"),
    ] {
        let mut spec = terminals(); spec[field] = json!([entry]);
        assert_eq!(pcb::parse(&spec.to_string()).unwrap_err(), error);
    }
}

const SMD: &str = r#"(footprint "Imported SOT-23"
  (unknown (ignored "text with (parentheses)"))
  (fp_rect (start -2 -2) (end 2 2) (layer "F.CrtYd"))
  (fp_poly (pts (xy -0.8 -1.5) (xy 0.8 -1.5) (xy 0.8 1.5) (xy -0.8 1.5)) (layer "F.Fab"))
  (pad "1" smd rect (at -1 -1) (size 0.8 0.8) (layers "F.Cu" "F.Paste"))
  (pad "2" smd roundrect (at -1 1 90) (size 0.8 1) (roundrect_rratio 0.2) (layers "F.Cu"))
  (pad "3" smd oval (at 1 0) (size 0.8 1) (layers "B.Cu")))"#;

#[tokio::test]
async fn kicad_smd_y_flip_roundrect_and_bottom_pad_build_cleanly() {
    let (name, footprint) = pcb::kicad::parse(SMD).unwrap();
    assert_eq!(name, "Imported SOT-23");
    assert_eq!(footprint["pads"][0]["at"], json!([-1,1]));
    assert_eq!(footprint["pads"][1]["at"], json!([-1,-1]));
    assert_eq!(footprint["pads"][1]["size"], json!([0.8,1]));
    assert_eq!(footprint["pads"][1]["roundrect_rratio"], 0.2);
    assert_eq!(footprint["pads"][2]["layers"], "bottom");
    assert_eq!(footprint["courtyard"], json!([[-2,-2],[2,-2],[2,2],[-2,2]]));
    let spec = json!({"board":{"width":20,"height":16},"footprints":{name.clone():footprint},"parts":[
        {"ref":"U1","kind":"chip","footprint":name,"at":[0,0]}]});
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&built);
    let p1 = elements.iter().find(|e| e["pcb_smtpad_id"] == "U1.1").unwrap();
    assert_eq!(p1["x"], -1); assert_eq!(p1["y"], 1);
    let p2 = elements.iter().find(|e| e["pcb_smtpad_id"] == "U1.2").unwrap();
    assert_eq!(p2["rotation"], 90); assert_eq!(p2["rect_border_radius"], 0.16);
    assert_eq!(elements.iter().find(|e| e["pcb_smtpad_id"] == "U1.3").unwrap()["layer"], "bottom");
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[tokio::test]
async fn legacy_through_hole_oval_drill_and_np_through_hole_build_cleanly() {
    let (name, footprint) = pcb::kicad::parse(r#"(module Legacy
        (fp_line (start -4 -3) (end 4 3) (layer F.CrtYd))
        (pad 1 thru_hole oval (at -2 1) (size 2 2.5) (drill oval 0.8 1.2) (layers *.Cu *.Mask))
        (pad 2 thru_hole circle (at 2 -1) (size 2 2) (drill 0.8) (layers *.Cu))
        (pad "" np_thru_hole circle (at 0 0) (size 1 1) (drill 1) (layers *.Cu)))"#).unwrap();
    assert_eq!(footprint["pads"][0]["at"], json!([-2,-1]));
    assert_eq!(footprint["pads"][0]["size"], json!([2,2.5]));
    assert_eq!(footprint["pads"][0]["drill"], json!([0.8,1.2]));
    assert_eq!(footprint["pads"][1]["drill"], 0.8);
    assert_eq!(footprint["pads"][2]["plated"], false);
    let spec = json!({"board":{"width":20,"height":16},"footprints":{name.clone():footprint},
        "parts":[{"ref":"J1","kind":"header","footprint":name,"at":[0,0]}]});
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[tokio::test]
async fn rectangular_rotated_through_pad_keeps_dimensions_and_slot() {
    let (name, footprint) = pcb::kicad::parse(r#"(footprint RectTHT
        (pad 1 thru_hole rect (at 0 1 45) (size 2 3) (drill oval 0.8 1.2) (layers *.Cu)))"#).unwrap();
    let spec = json!({"board":{"width":20,"height":16},"footprints":{name.clone():footprint},
        "parts":[{"ref":"J1","kind":"header","footprint":name,"at":[0,0]}]});
    let built = pcb::build(&spec.to_string(), "test", "pcb/test.pcb.json").unwrap();
    let elements = circuit(&built);
    let pad = elements.iter().find(|e| e["type"] == "pcb_plated_hole").unwrap();
    assert_eq!(pad["shape"], "rect");
    assert_eq!(pad["outer_width"], 2); assert_eq!(pad["outer_height"], 3);
    assert_eq!(pad["hole_width"], 0.8); assert_eq!(pad["hole_height"], 1.2);
    assert_eq!(pad["rotation"], 45); assert_eq!(pad["y"], -1);
    assert!(built.files.iter().find(|(p,_)| p.ends_with("-PTH.drl")).unwrap().1.contains("G85"));
    let report = drc(&spec, &built).await;
    assert!(report.ends_with("No DRC errors."), "{report}");
}

#[test]
fn custom_kicad_pad_primitives_become_the_bounding_rectangle() {
    let (_, footprint) = pcb::kicad::parse(r#"(footprint Custom
        (pad 1 smd custom (at 5 2) (size 1 1) (layers F.Cu)
            (primitives (gr_poly (pts (xy -2 -1) (xy 4 -1) (xy 4 3) (xy -2 3)) (width 0)))))"#).unwrap();
    let pad = &footprint["pads"][0];
    assert_eq!(pad["shape"], "rect");
    assert_eq!(pad["size"], json!([6,4]));
    assert_eq!(pad["at"], json!([6,-3]));
}

#[test]
fn kicad_errors_and_builtin_clashes_are_exact() {
    for (text, error) in [
        ("(kicad_pcb (version 1))", "kicad: not a footprint file"),
        ("(footprint X (pad 1 smd rect (at 0 0)))", "kicad: pad 1 has no size"),
        ("(footprint X", "kicad: unbalanced parentheses"),
        ("(footprint X))", "kicad: unbalanced parentheses"),
        ("(footprint \"X)", "kicad: unbalanced parentheses"),
    ] { assert_eq!(pcb::kicad::parse(text).unwrap_err(), error); }
    let (_, footprint) = pcb::kicad::parse(SMD).unwrap();
    let mut spec = terminals(); spec["footprints"] = json!({"0603":footprint});
    assert_eq!(pcb::parse(&spec.to_string()).unwrap_err(), "footprints.0603: clashes with a built-in footprint");
}

#[tokio::test]
async fn import_tool_preserves_order_unknown_fields_and_checks_project_paths() {
    let original = r#"{"board":{"width":20,"height":16},"metadata":"keep me","parts":[]}"#;
    let project = support::project("batch6-import", &[("pcb/test.pcb.json", original), ("parts/custom.kicad_mod", SMD)]);
    let result = support::run("pcb_import_footprint", json!({"file":"parts/custom.kicad_mod","board":"pcb/test.pcb.json","name":"custom"}), &project.path).await.unwrap();
    assert!(result.text.contains("3 pad(s), 2.9 × 2.8 mm"), "{}", result.text);
    let text = std::fs::read_to_string(project.path.join("pcb/test.pcb.json")).unwrap();
    let spec: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(spec["metadata"], "keep me");
    assert_eq!(spec.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), ["board","metadata","parts","footprints"]);
    assert!(text.contains("\n  \"board\": {") && !text.contains("1.0") && !text.contains("-0.0"));
    for field in ["file", "board"] {
        let mut input = json!({"file":"parts/custom.kicad_mod","board":"pcb/test.pcb.json"});
        input[field] = json!("../outside.kicad_mod");
        assert!(support::run("pcb_import_footprint", input, &project.path).await.is_err());
    }
}
