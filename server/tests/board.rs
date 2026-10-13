mod common;
#[path = "common/tools.rs"]
mod support;

use common::TempDir;
use forge_server::tools::{
    all_tools,
    board::{node_program, pcb_engine, run_engine, summarize_warnings},
    exec::on_path,
    fit::{
        apply, fit_check, parse_stl, rotation_matrix, triangle_hits_box, unapply, BoardBoxes,
        FitBoard, FitSolid, Placement, Vec3,
    },
    ToolContext,
};
use serde_json::{json, Value};
use support::{app_dir, project, run};

/** The 12 triangles of an axis-aligned box. */
fn box_triangles(min: Vec3, max: Vec3) -> Vec<f32> {
    let c = |i: usize| {
        [0, 1, 2].map(|k| {
            if (i >> k) & 1 != 0 {
                max[k] as f32
            } else {
                min[k] as f32
            }
        })
    };
    [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ]
    .into_iter()
    .flat_map(|[a, b, d, e]| [c(a), c(b), c(d), c(a), c(d), c(e)].into_iter().flatten())
    .collect()
}

fn binary_stl(numbers: &[f32]) -> Vec<u8> {
    let count = numbers.len() / 9;
    let mut buf = vec![0; 84 + count * 50];
    buf[80..84].copy_from_slice(&(count as u32).to_le_bytes());
    for i in 0..count {
        for k in 0..9 {
            let at = 84 + i * 50 + 12 + k * 4;
            buf[at..at + 4].copy_from_slice(&numbers[i * 9 + k].to_le_bytes());
        }
    }
    buf
}

fn ascii_stl(numbers: &[f32]) -> String {
    let mut lines = vec!["solid t".into()];
    for tri in numbers.chunks_exact(9) {
        lines.extend([" facet normal 0 0 0".into(), "  outer loop".into()]);
        for p in tri.chunks_exact(3) {
            lines.push(format!("   vertex {} {} {}", p[0], p[1], p[2]));
        }
        lines.extend(["  endloop".into(), " endfacet".into()]);
    }
    lines.join("\n") + "\nendsolid t\n"
}

// A 40 × 30 × 1.6 mm board with a 5 mm tall connector J1 near its +x edge.
fn board() -> Value {
    json!({"size":[40,30,1.6],"center":[0,0],"components":[{"name":"J1","layer":"top","min":[14,-3,0.8],"max":[19,3,5.8]}]})
}

#[test]
fn stl_files_parse_whether_binary_or_ascii() {
    let tris = box_triangles([0.; 3], [1., 2., 3.]);
    assert_eq!(parse_stl(&binary_stl(&tris)).unwrap(), tris);
    assert_eq!(parse_stl(ascii_stl(&tris).as_bytes()).unwrap(), tris);
    assert!(parse_stl(b"hello")
        .unwrap_err()
        .to_string()
        .contains("not an STL"));
}

#[test]
fn rotations_follow_the_3d_views_xyz_order_and_undo_exactly() {
    let m = rotation_matrix([0., 0., 90.]);
    for (v, expected) in apply(m, [0.; 3], [1., 0., 0.])
        .into_iter()
        .zip([0., 1., 0.])
    {
        assert!((v - expected).abs() < 1e-9);
    }
    let r = rotation_matrix([30., -45., 120.]);
    let p = [3., -7., 11.];
    for (v, expected) in unapply(r, [1., 2., 3.], apply(r, [1., 2., 3.], p))
        .into_iter()
        .zip(p)
    {
        assert!((v - expected).abs() < 1e-9);
    }
}

#[test]
fn a_triangle_hits_a_box_only_when_they_overlap() {
    assert!(triangle_hits_box(
        [0.; 3],
        [1.; 3],
        [-5., -5., 0.],
        [5., -5., 0.],
        [0., 5., 0.]
    ));
    assert!(!triangle_hits_box(
        [0.; 3],
        [1.; 3],
        [-5., -5., 2.],
        [5., -5., 2.],
        [0., 5., 2.]
    ));
    // Its bounding box overlaps the box, but the slanted face passes by the corner.
    assert!(!triangle_hits_box(
        [0.; 3],
        [1.; 3],
        [1.5, 3., 0.],
        [3., 1.5, 0.],
        [3., 3., 0.]
    ));
}

#[test]
fn fit_check_finds_collisions_tight_spots_and_clear_fits() {
    let wall = |x, bottom| FitSolid {
        name: "wall.stl".into(),
        placement: Placement::default(),
        triangles: box_triangles([x, -20., bottom], [x + 2., 20., 10.]),
    };
    let mut boards = [FitBoard {
        name: "main.glb".into(),
        placement: Placement::default(),
        board: serde_json::from_value::<BoardBoxes>(board()).unwrap(),
    }];
    let issues = |board: &[FitBoard], solid: FitSolid| {
        serde_json::to_value(fit_check(board, &[solid], 0.5)).unwrap()
    };
    // A rib hanging down to 1.5 mm cuts through J1 (x 14..19) but clears the board's top (0.8).
    assert_eq!(
        issues(&boards, wall(17., 1.5)),
        json!([{"board":"main.glb","item":"J1","part":"wall.stl","status":"collision"}])
    );
    assert_eq!(
        issues(&boards, wall(20.3, -5.)),
        json!([{"board":"main.glb","item":"board","part":"wall.stl","status":"tight"}])
    );
    assert_eq!(issues(&boards, wall(25., -5.)), json!([]));
    // Turned 180° about Z, J1 sits at the -x edge instead.
    boards[0].placement.rotation = [0., 0., 180.];
    assert_eq!(issues(&boards, wall(17., 1.5)), json!([]));
    boards[0].placement = Placement::default();
    // A board resting on a standoff only touches it.
    let standoff = FitSolid {
        name: "post.stl".into(),
        placement: Placement::default(),
        triangles: box_triangles([-18., -13., -5.], [-15., -10., -0.8]),
    };
    assert_eq!(
        issues(&boards, standoff),
        json!([{"board":"main.glb","item":"board","part":"post.stl","status":"tight"}])
    );
}

#[tokio::test]
async fn design_assemble_writes_the_assembly_and_reports_the_fit() {
    let dir = project(
        "pcb",
        &[
            ("pcb/main.glb", b"glb".to_vec()),
            ("pcb/main.board.json", serde_json::to_vec(&board()).unwrap()),
            (
                "models/lid.stl",
                binary_stl(&box_triangles([-25., -20., 4.], [25., 20., 6.])),
            ),
            ("models/lid.step", b"ISO-10303-21;".to_vec()),
        ],
    );
    let low = run("design_assemble", json!({"name":"low","parts":[{"file":"pcb/main.glb"},{"file":"models/lid.step","color":"#8899aa","opacity":0.4}]}), &dir.path).await.unwrap();
    for expected in [
        "1 COLLISION",
        "J1 on pcb/main.glb cuts into models/lid.stl",
        "tallest part J1 reaches 5 mm above the board",
    ] {
        assert!(low.text.contains(expected), "{}", low.text);
    }
    assert_eq!(low.files, ["design/low.json"]);
    let saved = std::fs::read_to_string(dir.path.join("design/low.json")).unwrap();
    let design: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(
        design["parts"][1],
        json!({"file":"models/lid.stl","position":[0,0,0],"rotation":[0,0,0],"color":"#8899aa","opacity":0.4})
    );
    assert_eq!(design["fit"][0]["status"], "collision");
    // JSON.stringify preserves this key order and writes integers without .0.
    assert_eq!(saved, "{\n  \"parts\": [\n    {\n      \"file\": \"pcb/main.glb\",\n      \"position\": [\n        0,\n        0,\n        0\n      ],\n      \"rotation\": [\n        0,\n        0,\n        0\n      ]\n    },\n    {\n      \"file\": \"models/lid.stl\",\n      \"position\": [\n        0,\n        0,\n        0\n      ],\n      \"rotation\": [\n        0,\n        0,\n        0\n      ],\n      \"color\": \"#8899aa\",\n      \"opacity\": 0.4\n    }\n  ],\n  \"clearance\": 0.5,\n  \"fit\": [\n    {\n      \"board\": \"pcb/main.glb\",\n      \"item\": \"J1\",\n      \"part\": \"models/lid.stl\",\n      \"status\": \"collision\"\n    }\n  ]\n}\n");

    let high = run("design_assemble", json!({"name":"high","parts":[{"file":"pcb/main.glb"},{"file":"models/lid.stl","position":[0,0,3]}]}), &dir.path).await.unwrap();
    assert!(
        high.text.contains("everything clears by at least 0.5 mm"),
        "{}",
        high.text
    );
    assert!(
        high.text
            .contains("models/lid.stl spans x -25, 25, y -20, 20, z 7, 9 mm"),
        "{}",
        high.text
    );
}

#[tokio::test]
async fn design_assemble_writes_whole_numbers_and_negative_zero_like_javascript() {
    let dir = project(
        "pcb",
        &[("models/a.stl", binary_stl(&box_triangles([0.; 3], [1.; 3])))],
    );
    run("design_assemble", json!({"name":"numbers","parts":[{"file":"models/a.stl","position":[5.0,-0.0,1e-6],"rotation":[-0.0,1e21,1e-7],"opacity":1.0}],"clearance":5.0}), &dir.path).await.unwrap();
    let saved = std::fs::read_to_string(dir.path.join("design/numbers.json")).unwrap();
    assert_eq!(saved, "{\n  \"parts\": [\n    {\n      \"file\": \"models/a.stl\",\n      \"position\": [\n        5,\n        0,\n        0.000001\n      ],\n      \"rotation\": [\n        0,\n        1e+21,\n        1e-7\n      ],\n      \"opacity\": 1\n    }\n  ],\n  \"clearance\": 5,\n  \"fit\": []\n}\n");
}

#[tokio::test]
async fn design_assemble_refuses_bad_input() {
    let dir = project(
        "pcb",
        &[
            ("models/a.stl", binary_stl(&box_triangles([0.; 3], [1.; 3]))),
            ("notes.txt", b"x".to_vec()),
        ],
    );
    for (input, expected) in [
        (
            json!({"name":"a b","parts":[{"file":"models/a.stl"}]}),
            "name may only",
        ),
        (json!({"name":"a","parts":[]}), "1 to 50 parts"),
        (
            json!({"name":"a","parts":[{"file":"notes.txt"}]}),
            "must be .stl or .glb",
        ),
        (
            json!({"name":"a","parts":[{"file":"pcb/none.glb"}]}),
            "does not exist; build it first",
        ),
        (
            json!({"name":"a","parts":[{"file":"../a.stl"}]}),
            "outside the project",
        ),
        (
            json!({"name":"a","parts":[{"file":"models/a.stl","position":[1,2]}]}),
            "position must be three numbers",
        ),
        (
            json!({"name":"a","parts":[{"file":"models/a.stl","color":"red"}]}),
            "color must look like",
        ),
        (
            json!({"name":"a","parts":[{"file":"models/a.stl","opacity":0}]}),
            "opacity must be",
        ),
        (
            json!({"name":"a","parts":[{"file":"models/a.stl"}],"clearance":-1}),
            "clearance must be",
        ),
    ] {
        let err = run("design_assemble", input, &dir.path)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{err}");
    }
}

#[test]
fn engine_warnings_are_grouped_and_routine_ones_dropped() {
    let lines = summarize_warnings(
        &[
            "source_unnamed_trace_warning: trace has no name",
            "source_missing_manufacturer_part_number_warning: R1 has none",
            "source_missing_manufacturer_part_number_warning: R2 has none",
            "source_part_not_found_warning: R1 not found",
            "pcb_silkscreen_overlap_warning: R1 label overlaps C1",
            "pcb_silkscreen_overlap_warning: R2 label overlaps C2",
        ]
        .map(str::to_owned),
        false,
    );
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].starts_with("2 part(s) have no manufacturer part number")
            && lines[0].contains("lookup_parts: true")
    );
    assert_eq!(
        lines[1],
        "pcb_silkscreen_overlap_warning ×2, e.g. R1 label overlaps C1"
    );
    assert!(summarize_warnings(
        &["source_part_not_found_warning: R1 not found".into()],
        true
    )[0]
    .contains("R1 not found"));
}

fn has_engine() -> bool {
    if pcb_engine(&app_dir()).is_none() {
        eprintln!("skipped: PCB engine not built (npm run pcb-engine)");
        return false;
    }
    let settings = forge_server::Settings::from_env(app_dir());
    if !on_path(&node_program(settings.node_program.as_deref())) {
        eprintln!("skipped: node not installed (FORGE_NODE/PATH)");
        return false;
    }
    true
}

const LED_BOARD: &str = r#"export default () => (
  <board width="30mm" height="20mm">
    <resistor name="R1" resistance="1k" footprint="0402" pcbX={-6} pcbY={0} />
    <led name="LED1" color="red" footprint="0603" pcbX={6} pcbY={0} />
    <pinheader name="J1" pinCount={2} pitch="2.54mm" pcbX={0} pcbY={-6} />
    <trace from=".J1 > .pin1" to=".R1 > .pin1" />
    <trace from=".R1 > .pin2" to=".LED1 > .anode" />
    <trace from=".LED1 > .cathode" to=".J1 > .pin2" />
  </board>
)
"#;

#[tokio::test]
async fn pcb_build_json_validates_without_outputs_or_a_node_engine() {
    let dir = project("pcb_json", &[("pcb/led.pcb.json", r#"{
        "board":{"width":30,"height":20},
        "parts":[
            {"ref":"R1","kind":"resistor","footprint":"0402"},
            {"ref":"LED1","kind":"led","footprint":"0603"}
        ],
        "nets":{"A":["R1.2","LED1.A"]}
    }"#)]);
    let settings = forge_server::Settings::from_env(app_dir());
    let mut ctx = ToolContext::from_settings(dir.path.clone(), &settings);
    ctx.app_dir = dir.path.join("no-engine");
    ctx.node_program = Some(dir.path.join("no-node"));
    let tool = all_tools().into_iter().find(|t| t.name == "pcb_build").unwrap();
    let out = (tool.run)(json!({"source":"pcb/led.pcb.json"}), ctx).await.unwrap();
    assert_eq!(out.text, "Validated pcb/led.pcb.json: 2 part(s), 1 named net(s). Native PCB outputs are not built yet.");
    assert!(out.files.is_empty());
    assert!(!dir.path.join("fab").exists());
    assert_eq!(std::fs::read_dir(dir.path.join("pcb")).unwrap().count(), 1);
}

#[tokio::test]
async fn pcb_build_routes_a_board_and_writes_its_views_3d_model_and_fab_files() {
    if !has_engine() {
        return;
    }
    let dir = project("pcb", &[("pcb/led.tsx", LED_BOARD)]);
    let out = run("pcb_build", json!({"source":"pcb/led.tsx"}), &dir.path)
        .await
        .unwrap();
    for expected in [
        "board 30 × 20 mm",
        "3 part(s): R1, LED1, J1",
        "No errors: every connection is routed",
    ] {
        assert!(out.text.contains(expected), "{}", out.text);
    }
    for f in [
        "pcb/led-pcb.svg",
        "pcb/led-schematic.svg",
        "pcb/led.glb",
        "pcb/led.circuit.json",
        "pcb/led.board.json",
        "fab/led/",
    ] {
        assert!(out.files.iter().any(|file| file == f), "{f}");
    }
    let data: Value =
        serde_json::from_slice(&std::fs::read(dir.path.join("pcb/led.board.json")).unwrap())
            .unwrap();
    assert_eq!(
        &data["size"].as_array().unwrap()[..2],
        &[json!(30), json!(20)]
    );
    let mut names = data["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["J1", "LED1", "R1"]);
    let fab = std::fs::read_dir(dir.path.join("fab/led"))
        .unwrap()
        .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        fab.iter().any(|f| f.ends_with(".gbr"))
            && fab.iter().any(|f| f == "led-PTH.drl")
            && fab.iter().any(|f| f == "led-pnp.csv"),
        "{}",
        fab.join(", ")
    );
    let bom = std::fs::read_to_string(dir.path.join("fab/led/led-bom.csv")).unwrap();
    assert!(bom.starts_with("Comment,Designator,Footprint,LCSC Part #"));
    assert!(bom.lines().any(|l| l
        .split_once("1k")
        .is_some_and(|(_, tail)| tail.contains("R1"))));
    assert_eq!(
        &std::fs::read(dir.path.join("pcb/led.glb")).unwrap()[..4],
        b"glTF"
    );
}

#[tokio::test]
async fn pcb_build_pours_a_ground_plane() {
    if !has_engine() {
        return;
    }
    let source = LED_BOARD.replace("</board>", "  <trace from=\".J1 > .pin2\" to=\"net.GND\" />\n    <copperpour connectsTo=\"net.GND\" layer=\"bottom\" />\n  </board>");
    let dir = project("pcb", &[("pcb/gnd.tsx", source)]);
    let out = run("pcb_build", json!({"source":"pcb/gnd.tsx"}), &dir.path)
        .await
        .unwrap();
    assert!(
        out.text.contains("Copper pours: GND on bottom"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("engine_step_error"), "{}", out.text);
    assert!(
        std::fs::read_to_string(dir.path.join("fab/gnd/gnd-B_Cu.gbr"))
            .unwrap()
            .contains("G36")
    );
}

#[tokio::test]
async fn pcb_build_accepts_a_printed_antennas_open_arm_and_clears_ground_under_a_keepout() {
    if !has_engine() {
        return;
    }
    let dir = project(
        "pcb",
        &[(
            "pcb/ant.tsx",
            r#"export default () => (
  <board width="50mm" height="40mm">
    <antenna name="ANT1" antennaShape="2.4ghz_quarter_wave_monopole" pcbX={-20} pcbY={18.4} />
    <keepout pcbX={-3} pcbY={16} width="32mm" height="8mm" shape="rect" layers={["bottom"]} />
    <copperpour connectsTo="net.GND" layer="bottom" />
  </board>
)"#,
        )],
    );
    let out = run("pcb_build", json!({"source":"pcb/ant.tsx"}), &dir.path)
        .await
        .unwrap();
    assert!(out.text.contains("No errors"), "{}", out.text);
    assert!(!out.text.contains("disconnected endpoint"), "{}", out.text);
    let circuit: Value =
        serde_json::from_slice(&std::fs::read(dir.path.join("pcb/ant.circuit.json")).unwrap())
            .unwrap();
    let pour = circuit
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["type"] == "pcb_copper_pour")
        .unwrap();
    let max_y = pour["brep_shape"]["outer_ring"]["vertices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["x"].as_f64().unwrap() > -15. && v["x"].as_f64().unwrap() < 10.)
        .map(|v| v["y"].as_f64().unwrap())
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(max_y <= 12.1, "ground reaches y {max_y} under the antenna");
}

#[tokio::test]
async fn pcb_build_reports_board_errors_and_keeps_board_code_sandboxed() {
    if !has_engine() {
        return;
    }
    let outside = TempDir::new("outside");
    let dir = project("pcb", &[
        ("pcb/env.tsx", "throw new Error(\"KEY=\" + (process.env.FORGE_TEST_SECRET ?? \"hidden\"))\nexport default () => <board width=\"10mm\" height=\"10mm\" />\n".to_owned()),
        ("pcb/escape.tsx", format!("const fs = process.getBuiltinModule(\"node:fs\")\nlet r = \"written\"\ntry {{ fs.writeFileSync({}, \"x\") }} catch {{ r = \"blocked\" }}\nthrow new Error(\"WRITE=\" + r)\nexport default () => <board width=\"10mm\" height=\"10mm\" />\n", json!(outside.path.join("x.txt")))),
    ]);
    let build = all_tools()
        .into_iter()
        .find(|t| t.name == "pcb_build")
        .unwrap();
    let mut ctx = ToolContext::new(dir.path.clone(), app_dir());
    ctx.node_env
        .push(("FORGE_TEST_SECRET".into(), "s3cret".into()));
    for (input, expected) in [
        (json!({"source":"pcb/env.tsx"}), "KEY=hidden"),
        (json!({"source":"pcb/escape.tsx"}), "WRITE=blocked"),
    ] {
        let err = (build.run)(input, ctx.clone())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{err}");
    }
    assert_eq!(std::fs::read_dir(&outside.path).unwrap().count(), 0);
    for (input, expected) in [
        (json!({"source":"pcb/none.tsx"}), "does not exist"),
        (
            json!({"source":"pcb/env.tsx","name":"a/b"}),
            "name may only",
        ),
    ] {
        let err = (build.run)(input, ctx.clone())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains(expected), "{err}");
    }
}

#[tokio::test]
async fn pcb_engine_uses_configured_node_and_only_path_lang_and_args() {
    use std::os::unix::fs::PermissionsExt;
    let dir = project("node", &[("node-stub", "#!/bin/sh\nprintf '%s\\n' \"$PATH\" \"$LANG\" \"${FORGE_TEST_SECRET-hidden}\" \"$FORGE_ARGS\" \"$@\"\n")]);
    let stub = dir.path.join("node-stub");
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut ctx = ToolContext::new(dir.path.clone(), app_dir());
    ctx.node_program = Some(stub);
    ctx.node_env = vec![
        ("PATH".into(), "/configured/path".into()),
        ("LANG".into(), "configured.UTF-8".into()),
        ("FORGE_TEST_SECRET".into(), "s3cret".into()),
    ];
    let engine = dir.path.join("engine.mjs");
    let result = run_engine(&engine, &ctx, json!({"name":"test"}), 1000)
        .await
        .unwrap();
    assert_eq!(result.code, 0, "{}", result.stderr);
    let lines = result.stdout.lines().collect::<Vec<_>>();
    assert_eq!(
        &lines[..4],
        &[
            "/configured/path",
            "configured.UTF-8",
            "hidden",
            "{\"name\":\"test\"}"
        ]
    );
    assert_eq!(
        &lines[4..],
        [
            "--permission".to_owned(),
            format!("--allow-fs-read={}", engine.display()),
            format!("--allow-fs-read={}", dir.path.display()),
            format!("--allow-fs-write={}", dir.path.display()),
            "--disable-warning=ExperimentalWarning".into(),
            "--disable-warning=SecurityWarning".into(),
            engine.to_string_lossy().into_owned()
        ]
    );
}
