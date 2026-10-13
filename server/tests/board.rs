mod common;
#[path = "common/tools.rs"]
mod support;

use forge_server::tools::{
    all_tools,
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

#[tokio::test]
async fn pcb_build_json_needs_no_node_engine() {
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
    let tool = all_tools().into_iter().find(|t| t.name == "pcb_build").unwrap();
    let out = (tool.run)(json!({"source":"pcb/led.pcb.json"}), ctx).await.unwrap();
    assert!(out.text.contains("pcb/led.pcb.json"), "{}", out.text);
}

#[tokio::test]
async fn pcb_build_rejects_tsx_and_jsx_sources() {
    let dir = project("pcb_legacy", &[("pcb/old.tsx", ""), ("pcb/old.jsx", "")]);
    for source in ["pcb/old.tsx", "pcb/old.jsx"] {
        let err = run("pcb_build", json!({"source":source}), &dir.path)
            .await
            .unwrap_err()
            .to_string();
        assert_eq!(err, "source must be a .pcb.json file");
    }
}
