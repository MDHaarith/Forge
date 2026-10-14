mod common;
#[path = "common/tools.rs"]
mod support;

use forge_server::tools::pcb;
use serde_json::{json, Value};

fn spec() -> Value {
    json!({"board":{"width":20,"height":15},"parts":[
        {"ref":"J1","kind":"header","footprint":"pinheader_1x2","at":[-5,-2]},
        {"ref":"R1","kind":"resistor","footprint":"0603","at":[5,2],"side":"bottom"}],
        "holes":[{"at":[0,5],"d":1}],"nets":{"N":["J1.1","R1.1"]}})
}

fn file<'a>(built: &'a pcb::NativeBuild, path: &str) -> &'a str {
    &built.files.iter().find(|(p, _)| p == path).unwrap_or_else(|| panic!("missing {path}")).1
}

fn drills(text: &str) -> usize { text.lines().filter(|l| l.starts_with('X')).count() }

#[test]
fn two_by_three_tabs_panel_steps_all_layers_drills_and_assembly() {
    let mut input = spec();
    let single = pcb::build(&input.to_string(), "test", "pcb/test.pcb.json").unwrap();
    input["panel"] = json!({"rows":2,"cols":3});
    let built = pcb::build(&input.to_string(), "test", "pcb/test.pcb.json").unwrap();
    assert!(built.report.contains("Panel: 2 x 3 boards, 64 x 42 mm, tabs."));
    for side in ["F", "B"] {
        let board = format!("fab/test/test-{side}_Cu.gbr");
        let panel = format!("fab/test-panel/test-panel-{side}_Cu.gbr");
        assert_eq!(file(&built, &panel).matches("D03*").count(), 6 * file(&single, &board).matches("D03*").count() + 3);
        for layer in ["Mask", "Paste", "Silkscreen"] {
            let board = format!("fab/test/test-{side}_{layer}.gbr");
            let panel = format!("fab/test-panel/test-panel-{side}_{layer}.gbr");
            assert_eq!(file(&built, &panel).matches("D03*").count(), 6 * file(&single, &board).matches("D03*").count() + if layer == "Mask" { 3 } else { 0 });
        }
    }
    let edge = file(&built, "fab/test-panel/test-panel-Edge_Cuts.gbr");
    assert!(edge.contains("X0Y0D02*\nX64000000Y0D01*\nX64000000Y42000000D01*\nX0Y42000000D01*\nX0Y0D01*\n"));
    assert!(edge.matches("D02*").count() > 1);
    assert_eq!(drills(file(&built, "fab/test-panel/test-panel-PTH.drl")), 6 * drills(file(&single, "fab/test/test-PTH.drl")));
    let npth = file(&built, "fab/test-panel/test-panel-NPTH.drl");
    assert!(npth.contains("C0.5\n") && npth.contains("C2\n"));
    assert_eq!(drills(npth), 6 * drills(file(&single, "fab/test/test-NPTH.drl")) + 160 + 3);
    let pnp = file(&built, "fab/test-panel/test-panel-pnp.csv");
    assert_eq!(pnp.lines().count(), 1 + 6 * 2);
    assert!(pnp.contains("J1_r1c1,5,10.5,Top,0\n"));
    assert!(pnp.contains("R1_r2c3,59,31.5,Bottom,0\n"));
    assert!(file(&built, "fab/test/test-fab.svg").contains("Panel: 2 x 3 boards, 64 x 42 mm, tabs."));
    for (path, data) in &single.files {
        if !path.ends_with("-fab.svg") { assert_eq!(file(&built, path), data, "single-board file changed: {path}"); }
    }
    let again = pcb::build(&input.to_string(), "test", "pcb/test.pcb.json").unwrap();
    assert_eq!(built.files, again.files);
    assert_eq!(built.binary_files, again.binary_files);
}

#[test]
fn vscore_abuts_boards_and_scores_full_panel_and_rail_boundaries() {
    let mut input = spec(); input["panel"] = json!({"rows":2,"cols":3,"separation":"vscore"});
    let built = pcb::build(&input.to_string(), "test", "pcb/test.pcb.json").unwrap();
    assert!(built.report.contains("Panel: 2 x 3 boards, 60 x 40 mm, vscore."));
    let score = file(&built, "fab/test-panel/test-panel-VScore.gbr");
    assert!(score.contains("%TF.FileFunction,Other,VScore*%"));
    assert_eq!(score.matches("D01*").count(), 5);
    assert!(score.contains("X20000000Y0D02*\nX20000000Y40000000D01*\n"));
    assert!(score.contains("X0Y20000000D02*\nX60000000Y20000000D01*\n"));
    assert_eq!(drills(file(&built, "fab/test-panel/test-panel-NPTH.drl")), 6 + 3);
    assert!(file(&built, "fab/test/test-fab.svg").contains("test-panel-VScore.gbr"));
}

#[test]
fn validation_errors_and_boolean_fields_stay_strict() {
    for (field, value, error) in [
        ("rows", json!(0), "panel.rows must be 1..20"), ("rows", json!(21), "panel.rows must be 1..20"),
        ("rows", json!(-1), "panel.rows must be 1..20"),
        ("cols", json!(0), "panel.cols must be 1..20"), ("cols", json!(21), "panel.cols must be 1..20"),
        ("cols", json!(-1), "panel.cols must be 1..20"),
        ("spacing", json!(0.5), "panel: tabs need spacing of at least 1 mm"),
        ("spacing", json!(-1), "panel.spacing must be nonnegative"),
        ("rails", json!(-1), "panel.rails must be nonnegative"),
        ("rails", json!(0), "panel: fiducials and tooling holes need rails"),
        ("tab_width", json!(0), "panel.tab_width must be positive"),
        ("tabs", json!(0), "panel.tabs must be positive"),
        ("tabs", json!(65), "panel.tabs must be at most 64"),
        ("separation", json!("laser"), "panel.separation must be tabs or vscore"),
        ("rail_sides", json!("z"), "panel.rail_sides must be x, y or both"),
    ] {
        let mut input = spec(); input["panel"] = json!({"rows":2,"cols":3}); input["panel"][field] = value;
        assert_eq!(pcb::parse(&input.to_string()).unwrap_err(), error);
    }
    for separation in ["tabs", "vscore"] {
        let input = json!({"board":{"outline":[[-10,-8],[10,-8],[0,8]]},"panel":{"rows":2,"cols":3,"separation":separation}});
        assert_eq!(pcb::parse(&input.to_string()).unwrap_err(), format!("panel: {separation} need{} a rectangular board", if separation == "vscore" { "s" } else { "" }));
    }
    for field in ["mousebites", "fiducials", "tooling"] {
        for bad in [json!(1), json!("true"), json!(null)] {
            let mut input = spec(); input["panel"] = json!({"rows":2,"cols":3}); input["panel"][field] = bad;
            assert!(pcb::parse(&input.to_string()).is_err());
        }
    }
    let mut input = spec(); input["rules"] = json!({"paste_reduction":-0.05});
    assert_eq!(pcb::parse(&input.to_string()).unwrap_err(), "rules.paste_reduction must be nonnegative");
    for bad in [json!(1), json!("true"), json!(null)] {
        input["rules"] = json!({"paste_windowpane":bad}); assert!(pcb::parse(&input.to_string()).is_err());
    }
    input["rules"] = json!({}); input["panel"] = json!({"rows":1,"cols":1,"rails":0,"fiducials":false,"tooling":false});
    assert!(pcb::parse(&input.to_string()).is_ok());
}

#[tokio::test]
async fn pcb_build_writes_new_and_panel_files_through_project_paths() {
    let mut input = spec(); input["panel"] = json!({"rows":2,"cols":3,"rail_sides":"x"});
    let project = support::project("batch5", &[("pcb/test.pcb.json", input.to_string())]);
    let result = support::run("pcb_build", json!({"source":"pcb/test.pcb.json"}), &project.path).await.unwrap();
    for path in &result.files { assert!(project.path.join(path).is_file(), "missing written {path}"); }
    for path in ["fab/test/test.ipc", "fab/test/test-fab.svg", "fab/test-panel/test-panel-Edge_Cuts.gbr", "fab/test-panel/test-panel-pnp.csv"] {
        assert!(result.files.iter().any(|p| p == path));
    }
    assert!(result.text.contains("Panel: 2 x 3 boards, 74 x 32 mm, tabs."));
}
