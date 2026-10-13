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

/// Counts `<path class="hidden"` inside `<g id="view-front">` in SVG text.
fn count_hidden_paths_in_front_view(svg: &str) -> usize {
    let Some(start) = svg.find("<g id=\"view-front\">") else {
        return 0;
    };
    let rest = &svg[start + "<g id=\"view-front\">".len()..];
    let end = rest
        .find("<g id=\"view-")
        .unwrap_or_else(|| rest.find("</svg>").unwrap_or(rest.len()));
    let front_view = &rest[..end];
    front_view.matches("<path class=\"hidden\"").count()
}

/// Extracts text contents from `<text>` elements in SVG.
fn extract_text_contents(svg: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut cursor = svg;
    while let Some(start) = cursor.find("<text") {
        let after_tag_start = &cursor[start..];
        let Some(tag_close) = after_tag_start.find('>') else {
            break;
        };
        let inner_start = &after_tag_start[tag_close + 1..];
        let Some(end_tag) = inner_start.find("</text>") else {
            break;
        };
        let raw_text = &inner_start[..end_tag];
        let mut clean = String::new();
        let mut in_tag = false;
        for c in raw_text.chars() {
            if c == '<' {
                in_tag = true;
            } else if c == '>' {
                in_tag = false;
            } else if !in_tag {
                clean.push(c);
            }
        }
        let trimmed = clean.trim().to_string();
        if !trimmed.is_empty() {
            results.push(trimmed);
        }
        cursor = &inner_start[end_tag + "</text>".len()..];
    }
    results
}

/// Extracts coordinate bounds for X and Y in the ENTITIES section of DXF R12.
fn dxf_coord_spans(dxf: &str) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let entities_part = if let Some(idx) = dxf.find("ENTITIES") {
        &dxf[idx..]
    } else {
        dxf
    };
    let lines: Vec<&str> = entities_part.lines().map(str::trim).collect();
    for i in 0..lines.len().saturating_sub(1) {
        if lines[i] == "10" || lines[i] == "11" {
            if let Ok(val) = lines[i + 1].parse::<f64>() {
                min_x = min_x.min(val);
                max_x = max_x.max(val);
            }
        } else if lines[i] == "20" || lines[i] == "21" {
            if let Ok(val) = lines[i + 1].parse::<f64>() {
                min_y = min_y.min(val);
                max_y = max_y.max(val);
            }
        }
    }
    (min_x, max_x, min_y, max_y)
}

// =========================================================================
// 3.1 loft
// =========================================================================

#[tokio::test]
async fn loft_ruled_squares() {
    let (_dir, out) = build(json!({
        "name": "loft_sq",
        "steps": [{
            "op": "loft",
            "ruled": true,
            "sections": [
                {
                    "outline": {
                        "start": [-10, -10],
                        "segments": [
                            {"line": [10, -10]},
                            {"line": [10, 10]},
                            {"line": [-10, 10]},
                            {"line": [-10, -10]}
                        ]
                    },
                    "z": 0
                },
                {
                    "outline": {
                        "start": [-5, -5],
                        "segments": [
                            {"line": [5, -5]},
                            {"line": [5, 5]},
                            {"line": [-5, 5]},
                            {"line": [-5, -5]}
                        ]
                    },
                    "z": 10
                }
            ]
        }]
    }))
    .await;
    assert_eq!(volume(&out, 0), 2333.333);
    assert_eq!(out["bodies"][0]["bbox"], json!([[-10, -10, 0], [10, 10, 10]]));
}

#[tokio::test]
async fn loft_ruled_circles() {
    let (_dir, out) = build(json!({
        "name": "loft_circ",
        "steps": [{
            "op": "loft",
            "ruled": true,
            "sections": [
                {"outline": {"circle": [0, 0, 10]}, "z": 0},
                {"outline": {"circle": [0, 0, 5]}, "z": 10}
            ]
        }]
    }))
    .await;
    assert_eq!(volume(&out, 0), 1832.596);
}

#[tokio::test]
async fn loft_smooth_squares() {
    let square = |half: f64| {
        json!({
            "start": [-half, -half],
            "segments": [
                {"line": [half, -half]},
                {"line": [half, half]},
                {"line": [-half, half]},
                {"line": [-half, -half]}
            ]
        })
    };
    let (_dir, out) = build(json!({
        "name": "loft_smooth",
        "steps": [{
            "op": "loft",
            "sections": [
                {"outline": square(10.0), "z": 0},
                {"outline": square(5.0), "z": 10},
                {"outline": square(10.0), "z": 20}
            ]
        }]
    }))
    .await;
    assert_eq!(volume(&out, 0), 3733.333);
}

#[tokio::test]
async fn loft_error_sections_count_or_z() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run(
        "part_build",
        json!({
            "name": "t",
            "steps": [{
                "op": "loft",
                "sections": [
                    {"outline": {"circle": [0, 0, 5]}, "z": 10},
                    {"outline": {"circle": [0, 0, 5]}, "z": 5}
                ]
            }]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("step 0: loft needs at least 2 sections with increasing z"),
        "{err}"
    );
}

#[tokio::test]
async fn loft_error_failed() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let branched = json!({
        "start": [0, 0],
        "segments": [
            {"line": [10, 0]},
            {"line": [0, 0]},
            {"line": [0, 10]},
            {"line": [0, 0]}
        ]
    });
    let err = run(
        "part_build",
        json!({
            "name": "t",
            "steps": [{
                "op": "loft",
                "sections": [
                    {"outline": branched, "z": 0},
                    {"outline": {"circle": [0, 0, 5]}, "z": 10}
                ]
            }]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("step 0: loft failed:"), "{err}");
}

// =========================================================================
// 3.1 sweep
// =========================================================================

#[tokio::test]
async fn sweep_line_path() {
    let (_dir, out) = build(json!({
        "name": "sw_line",
        "steps": [{
            "op": "sweep",
            "outline": {"circle": [0, 0, 2]},
            "path": {
                "start": [0, 0, 0],
                "segments": [{"line": [30, 0, 0]}]
            }
        }]
    }))
    .await;
    assert_eq!(volume(&out, 0), 376.991);
}

#[tokio::test]
async fn sweep_arc_path() {
    let (_dir, out) = build(json!({
        "name": "sw_arc",
        "steps": [{
            "op": "sweep",
            "outline": {"circle": [0, 0, 2]},
            "path": {
                "start": [20, 0, 0],
                "segments": [{
                    "arc": [0, 20, 0],
                    "through": [14.142136, 14.142136, 0]
                }]
            }
        }]
    }))
    .await;
    assert_eq!(volume(&out, 0), 394.784);
}

#[tokio::test]
async fn sweep_line_arc_line_path() {
    let (_dir, out) = build(json!({
        "name": "sw_comp",
        "steps": [{
            "op": "sweep",
            "outline": {"circle": [0, 0, 2]},
            "path": {
                "start": [0, 0, 0],
                "segments": [
                    {"line": [10, 0, 0]},
                    {"arc": [20, 10, 0], "through": [17.071068, 2.928932, 0]},
                    {"line": [20, 20, 0]}
                ]
            }
        }]
    }))
    .await;
    assert_eq!(volume(&out, 0), 448.720);
}

#[tokio::test]
async fn sweep_error_not_tangential() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run(
        "part_build",
        json!({
            "name": "t",
            "steps": [{
                "op": "sweep",
                "outline": {"circle": [0, 0, 2]},
                "path": {
                    "start": [0, 0, 0],
                    "segments": [
                        {"line": [10, 0, 0]},
                        {"line": [10, 10, 0]}
                    ]
                }
            }]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("step 0: path segments must meet tangentially; join corners with an arc"),
        "{err}"
    );
}

#[tokio::test]
async fn sweep_error_failed() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run(
        "part_build",
        json!({
            "name": "t",
            "steps": [{
                "op": "sweep",
                "outline": {"circle": [0, 0, 50]},
                "path": {
                    "start": [1, 0, 0],
                    "segments": [{
                        "arc": [-1, 0, 0],
                        "through": [0, 0, 0]
                    }]
                }
            }]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("step 0: sweep failed:"), "{err}");
}

// =========================================================================
// 3.1 shell
// =========================================================================

#[tokio::test]
async fn shell_box_open_top() {
    let (_dir, out) = build(json!({
        "name": "sh_box",
        "steps": [
            {"id": "a", "op": "box", "size": [40, 30, 20]},
            {"op": "shell", "target": "a", "thickness": 2, "open": ["top"]}
        ]
    }))
    .await;
    assert_eq!(volume(&out, 0), 7152.0);
}

#[tokio::test]
async fn shell_cylinder_thickness() {
    let (_dir, out) = build(json!({
        "name": "sh_cyl",
        "steps": [
            {"id": "a", "op": "cylinder", "radius": 10, "height": 20},
            {"op": "shell", "target": "a", "thickness": 1}
        ]
    }))
    .await;
    assert_eq!(volume(&out, 0), 1448.274);
}

#[tokio::test]
async fn shell_error_no_face() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run(
        "part_build",
        json!({
            "name": "t",
            "steps": [
                {"id": "a", "op": "sphere", "radius": 10},
                {"op": "shell", "target": "a", "thickness": 1, "open": ["top"]}
            ]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("step 1: shell found no top face"), "{err}");
}

#[tokio::test]
async fn shell_error_failed() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run(
        "part_build",
        json!({
            "name": "t",
            "steps": [
                {"id": "a", "op": "box", "size": [10, 10, 10]},
                {"op": "shell", "target": "a", "thickness": 20, "open": ["top"]}
            ]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("step 1: shell failed:"), "{err}");
}

// =========================================================================
// 3.1 mirror
// =========================================================================

#[tokio::test]
async fn mirror_disjoint_keep() {
    let (_dir, out) = build(json!({
        "name": "m_disjoint",
        "steps": [
            {"id": "a", "op": "box", "size": [10, 10, 10], "at": [5, 0, 0]},
            {"op": "mirror", "target": "a", "plane": "x", "keep": true}
        ]
    }))
    .await;
    assert_eq!(volume(&out, 0), 2000.0);
    assert_eq!(out["bodies"][0]["solids"], 2);
    assert_eq!(out["bodies"][0]["bbox"], json!([[-15, 0, 0], [15, 10, 10]]));
}

#[tokio::test]
async fn mirror_touching_single_solid() {
    let (_dir, out) = build(json!({
        "name": "m_touch",
        "steps": [
            {"id": "a", "op": "box", "size": [10, 10, 10]},
            {"op": "mirror", "target": "a", "plane": "x"}
        ]
    }))
    .await;
    assert_eq!(volume(&out, 0), 2000.0);
    assert_eq!(out["bodies"][0]["solids"], 1);
}

#[tokio::test]
async fn mirror_replace_keep_false() {
    let (_dir, out) = build(json!({
        "name": "m_nokeep",
        "steps": [
            {"id": "a", "op": "box", "size": [10, 10, 10], "at": [5, 0, 0]},
            {"op": "mirror", "target": "a", "plane": "x", "keep": false}
        ]
    }))
    .await;
    assert_eq!(volume(&out, 0), 1000.0);
    assert_eq!(out["bodies"][0]["bbox"], json!([[-15, 0, 0], [-5, 10, 10]]));
}

// =========================================================================
// 3.1 pattern
// =========================================================================

#[tokio::test]
async fn pattern_linear_box() {
    let (_dir, out) = build(json!({
        "name": "pat_lin",
        "steps": [
            {"id": "a", "op": "box", "size": [10, 10, 10]},
            {"op": "pattern", "target": "a", "linear": {"step": [20, 0, 0], "count": 3}}
        ]
    }))
    .await;
    assert_eq!(volume(&out, 0), 3000.0);
    assert_eq!(out["bodies"][0]["solids"], 3);
    assert_eq!(out["bodies"][0]["bbox"], json!([[0, 0, 0], [50, 10, 10]]));
}

#[tokio::test]
async fn pattern_polar_cylinders() {
    let (_dir, out) = build(json!({
        "name": "pat_pol",
        "steps": [
            {"id": "a", "op": "cylinder", "radius": 2, "height": 10, "at": [10, 0, 0]},
            {"op": "pattern", "target": "a", "polar": {"count": 6}}
        ]
    }))
    .await;
    assert_eq!(volume(&out, 0), 753.982);
    assert_eq!(out["bodies"][0]["solids"], 6);
    assert_eq!(out["bodies"][0]["bbox"], json!([[-12, -10.66, 0], [12, 10.66, 10]]));
}

#[tokio::test]
async fn pattern_error_missing_linear_or_polar() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run(
        "part_build",
        json!({
            "name": "t",
            "steps": [
                {"id": "a", "op": "box", "size": [10, 10, 10]},
                {"op": "pattern", "target": "a"}
            ]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("step 1: pattern needs linear or polar"),
        "{err}"
    );
}

#[tokio::test]
async fn pattern_error_invalid_count() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run(
        "part_build",
        json!({
            "name": "t",
            "steps": [
                {"id": "a", "op": "box", "size": [10, 10, 10]},
                {"op": "pattern", "target": "a", "linear": {"step": [20, 0, 0], "count": 1}}
            ]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("step 1: count must be an integer from 2 to 100"),
        "{err}"
    );
}

// =========================================================================
// 3.1 thread
// =========================================================================

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn thread_external_m6_rod() {
    let (_dir, out) = build(json!({
        "name": "th_ext",
        "steps": [
            {"op": "thread", "size": "M6", "length": 10}
        ]
    }))
    .await;
    assert_eq!(out["bodies"][0]["solids"], 1);
    let v = volume(&out, 0);
    assert!((228.42..=230.72).contains(&v), "expected volume in 228.42..=230.72, got {v}");
    let bbox = &out["bodies"][0]["bbox"];
    near(bbox[0][0].as_f64().unwrap(), -3.0, 0.01);
    near(bbox[0][1].as_f64().unwrap(), -3.0, 0.01);
    near(bbox[0][2].as_f64().unwrap(), 0.0, 0.01);
    near(bbox[1][0].as_f64().unwrap(), 3.0, 0.01);
    near(bbox[1][1].as_f64().unwrap(), 3.0, 0.01);
    near(bbox[1][2].as_f64().unwrap(), 10.0, 0.01);
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn thread_internal_m6_hole() {
    let (_dir, out) = build(json!({
        "name": "th_int",
        "steps": [
            {"id": "a", "op": "box", "size": [20, 20, 10]},
            {"op": "hole", "target": "a", "points": [[10, 10]], "size": "M6", "kind": "thread"}
        ]
    }))
    .await;
    let removed = 4000.0 - volume(&out, 0);
    assert!((230.18..=232.49).contains(&removed), "expected removed volume in 230.18..=232.49, got {removed}");
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn thread_error_invalid_size() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run(
        "part_build",
        json!({
            "name": "t",
            "steps": [
                {"op": "thread", "size": "M7", "length": 10}
            ]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("step 0: thread size must be one of M2, M2.5, M3, M4, M5, M6, M8, M10, M12"),
        "{err}"
    );
}

// =========================================================================
// 3.1 sheet
// =========================================================================

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn sheet_single_bend_l_bracket() {
    let (_dir, out) = build(json!({
        "name": "sh_l",
        "steps": [{
            "op": "sheet",
            "thickness": 2,
            "width": 10,
            "flanges": [30, 20],
            "bends": [{"angle": 90, "radius": 2}],
            "k_factor": 0.44
        }]
    }))
    .await;
    assert_eq!(volume(&out, 0), 1094.248);
    assert_eq!(out["bodies"][0]["bbox"], json!([[0, 0, 0], [34, 10, 24]]));
    assert_eq!(out["bodies"][0]["flat"], json!([54.524, 10]));
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn sheet_double_bend_u_bracket() {
    let (_dir, out) = build(json!({
        "name": "sh_u",
        "steps": [{
            "op": "sheet",
            "thickness": 2,
            "width": 10,
            "flanges": [20, 30, 20],
            "bends": [{"angle": 90, "radius": 2}, {"angle": 90, "radius": 2}],
            "k_factor": 0.44
        }]
    }))
    .await;
    assert_eq!(volume(&out, 0), 1588.496);
    assert_eq!(out["bodies"][0]["bbox"], json!([[0, 0, 0], [24, 10, 38]]));
    assert_eq!(out["bodies"][0]["flat"], json!([79.049, 10]));
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn sheet_error_flanges_bends_mismatch() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run(
        "part_build",
        json!({
            "name": "t",
            "steps": [{
                "op": "sheet",
                "thickness": 2,
                "width": 10,
                "flanges": [30, 20],
                "bends": []
            }]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("step 0: sheet needs one more flange than bends"),
        "{err}"
    );
}

// =========================================================================
// 3.2 part_inspect additions
// =========================================================================

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn inspect_center_of_mass_box() {
    let (dir, _out) = build(json!({
        "name": "box234",
        "steps": [{"op": "box", "size": [2, 3, 4]}]
    }))
    .await;
    let report = run("part_inspect", json!({"path": "models/box234.step"}), &dir.path)
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    assert_eq!(report["center_of_mass"], json!([1, 1.5, 2]));
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn inspect_cubes_overlapping() {
    let dir = project("part", &[] as &[(&str, &str)]);
    run(
        "part_build",
        json!({
            "name": "cube_a",
            "steps": [{"op": "box", "size": [10, 10, 10]}]
        }),
        &dir.path,
    )
    .await
    .unwrap();
    run(
        "part_build",
        json!({
            "name": "cube_b",
            "steps": [{"op": "box", "size": [10, 10, 10], "at": [8, 0, 0]}]
        }),
        &dir.path,
    )
    .await
    .unwrap();
    let report = run(
        "part_inspect",
        json!({
            "path": "models/cube_a.step",
            "against": ["models/cube_b.step"]
        }),
        &dir.path,
    )
    .await
    .unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    let interference = report["interference"].as_array().unwrap();
    assert_eq!(interference.len(), 1);
    assert_eq!(interference[0]["a"], "models/cube_a.step#1");
    assert_eq!(interference[0]["b"], "models/cube_b.step#1");
    assert_eq!(interference[0]["overlap_volume"], 200);
    assert_eq!(interference[0]["clearance"], 0);
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn inspect_cubes_clearance_3() {
    let dir = project("part", &[] as &[(&str, &str)]);
    run(
        "part_build",
        json!({
            "name": "cube_a",
            "steps": [{"op": "box", "size": [10, 10, 10]}]
        }),
        &dir.path,
    )
    .await
    .unwrap();
    run(
        "part_build",
        json!({
            "name": "cube_b",
            "steps": [{"op": "box", "size": [10, 10, 10], "at": [13, 0, 0]}]
        }),
        &dir.path,
    )
    .await
    .unwrap();
    let report = run(
        "part_inspect",
        json!({
            "path": "models/cube_a.step",
            "against": ["models/cube_b.step"]
        }),
        &dir.path,
    )
    .await
    .unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    let interference = report["interference"].as_array().unwrap();
    assert_eq!(interference.len(), 1);
    assert_eq!(interference[0]["overlap_volume"], 0);
    assert_eq!(interference[0]["clearance"], 3);
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn inspect_cubes_clearance_5() {
    let dir = project("part", &[] as &[(&str, &str)]);
    run(
        "part_build",
        json!({
            "name": "cube_a",
            "steps": [{"op": "box", "size": [10, 10, 10]}]
        }),
        &dir.path,
    )
    .await
    .unwrap();
    run(
        "part_build",
        json!({
            "name": "cube_b",
            "steps": [{"op": "box", "size": [10, 10, 10], "at": [13, 14, 0]}]
        }),
        &dir.path,
    )
    .await
    .unwrap();
    let report = run(
        "part_inspect",
        json!({
            "path": "models/cube_a.step",
            "against": ["models/cube_b.step"]
        }),
        &dir.path,
    )
    .await
    .unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    let interference = report["interference"].as_array().unwrap();
    assert_eq!(interference.len(), 1);
    assert_eq!(interference[0]["overlap_volume"], 0);
    assert_eq!(interference[0]["clearance"], 5);
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn inspect_stl_tetrahedron() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let facet = |a: &str, b: &str, c: &str| {
        format!("facet normal 0 0 0\nouter loop\nvertex {a}\nvertex {b}\nvertex {c}\nendloop\nendfacet\n")
    };
    let (o, x, y, z) = ("0 0 0", "1 0 0", "0 1 0", "0 0 1");
    let ascii = format!(
        "solid t\n{}{}{}{}endsolid t\n",
        facet(o, y, x),
        facet(o, x, z),
        facet(o, z, y),
        facet(x, y, z)
    );
    std::fs::write(dir.path.join("tetra.stl"), ascii).unwrap();
    let report = run("part_inspect", json!({"path": "tetra.stl"}), &dir.path)
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    assert_eq!(report["center_of_mass"], json!([0.25, 0.25, 0.25]));
    assert_eq!(report["interference"], json!([]));
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn inspect_error_against_not_step() {
    let (dir, _out) = build(json!({
        "name": "cube",
        "steps": [{"op": "box", "size": [10, 10, 10]}]
    }))
    .await;
    std::fs::write(dir.path.join("other.txt"), "hello").unwrap();
    let err = run(
        "part_inspect",
        json!({
            "path": "models/cube.step",
            "against": ["other.txt"]
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("against files must be .step or .stp"),
        "{err}"
    );
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn inspect_error_overhang_range() {
    let (dir, _out) = build(json!({
        "name": "cube",
        "steps": [{"op": "box", "size": [10, 10, 10]}]
    }))
    .await;
    let err = run(
        "part_inspect",
        json!({
            "path": "models/cube.step",
            "overhang": 95
        }),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("overhang must be between 0 and 90"),
        "{err}"
    );
}

// =========================================================================
// 3.3 part_drawing
// =========================================================================

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn drawing_box_scale_1_1() {
    let (dir, _out) = build(json!({
        "name": "box403020",
        "steps": [{"op": "box", "size": [40, 30, 20]}]
    }))
    .await;
    let res = run(
        "part_drawing",
        json!({"path": "models/box403020.step"}),
        &dir.path,
    )
    .await
    .unwrap();
    let res_json: Value = serde_json::from_str(&res.text).unwrap();
    assert_eq!(res_json["sheet"], "A4");
    assert_eq!(res_json["scale"], "1:1");
    let svg = std::fs::read_to_string(dir.path.join(res_json["svg"].as_str().unwrap())).unwrap();
    assert_eq!(count_hidden_paths_in_front_view(&svg), 0);
    let texts = extract_text_contents(&svg);
    assert!(texts.iter().any(|t| t == "40"), "expected '40' in texts: {texts:?}");
    assert!(texts.iter().any(|t| t == "30"), "expected '30' in texts: {texts:?}");
    assert!(texts.iter().any(|t| t == "20"), "expected '20' in texts: {texts:?}");
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn drawing_box_with_hole_hidden_lines_and_table() {
    let (dir, _out) = build(json!({
        "name": "box_hole",
        "steps": [
            {"id": "a", "op": "box", "size": [40, 30, 20]},
            {"op": "hole", "target": "a", "points": [[20, 15]], "size": "M6", "kind": "clearance", "depth": 10}
        ]
    }))
    .await;
    let res = run(
        "part_drawing",
        json!({"path": "models/box_hole.step"}),
        &dir.path,
    )
    .await
    .unwrap();
    let res_json: Value = serde_json::from_str(&res.text).unwrap();
    let svg = std::fs::read_to_string(dir.path.join(res_json["svg"].as_str().unwrap())).unwrap();
    assert!(count_hidden_paths_in_front_view(&svg) > 0);
    let texts = extract_text_contents(&svg);
    assert!(texts.iter().any(|t| t == "H1"), "expected 'H1' in texts: {texts:?}");
    assert!(texts.iter().any(|t| t == "M6 clearance"), "expected 'M6 clearance' in texts: {texts:?}");
    assert!(texts.iter().any(|t| t == "10"), "expected '10' in texts: {texts:?}");
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn drawing_large_box_scale_1_2_a2() {
    let (dir, _out) = build(json!({
        "name": "box400300200",
        "steps": [{"op": "box", "size": [400, 300, 200]}]
    }))
    .await;
    let res = run(
        "part_drawing",
        json!({"path": "models/box400300200.step"}),
        &dir.path,
    )
    .await
    .unwrap();
    let res_json: Value = serde_json::from_str(&res.text).unwrap();
    assert_eq!(res_json["sheet"], "A2");
    assert_eq!(res_json["scale"], "1:2");
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn drawing_small_box_scale_5_1_a4() {
    let (dir, _out) = build(json!({
        "name": "box10105",
        "steps": [{"op": "box", "size": [10, 10, 5]}]
    }))
    .await;
    let res = run(
        "part_drawing",
        json!({"path": "models/box10105.step"}),
        &dir.path,
    )
    .await
    .unwrap();
    let res_json: Value = serde_json::from_str(&res.text).unwrap();
    assert_eq!(res_json["sheet"], "A4");
    assert_eq!(res_json["scale"], "5:1");
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn drawing_dxf_export() {
    let (dir, _out) = build(json!({
        "name": "box_dxf",
        "steps": [{"op": "box", "size": [40, 30, 20]}]
    }))
    .await;
    let res = run(
        "part_drawing",
        json!({"path": "models/box_dxf.step", "dxf": true}),
        &dir.path,
    )
    .await
    .unwrap();
    let res_json: Value = serde_json::from_str(&res.text).unwrap();
    let dxf_rel = res_json["dxf"].as_str().expect("dxf path in result");
    assert_eq!(dxf_rel, "drawings/box_dxf-top.dxf");
    let dxf = std::fs::read_to_string(dir.path.join(dxf_rel)).unwrap();
    assert!(dxf.contains("ENTITIES"), "DXF missing ENTITIES section");
    assert!(dxf.contains("OUTLINE"), "DXF missing OUTLINE layer");
    let line_count = dxf.lines().filter(|l| l.trim() == "LINE").count();
    assert!(line_count >= 4, "expected at least 4 LINE entities, got {line_count}");
    let (min_x, max_x, min_y, max_y) = dxf_coord_spans(&dxf);
    near(min_x, 0.0, 0.01);
    near(max_x, 40.0, 0.01);
    near(min_y, 0.0, 0.01);
    near(max_y, 30.0, 0.01);
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn drawing_error_path_not_step() {
    let dir = project("part", &[] as &[(&str, &str)]);
    std::fs::write(dir.path.join("file.txt"), "hello").unwrap();
    let err = run(
        "part_drawing",
        json!({"path": "file.txt"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("path must be a .step or .stp file"),
        "{err}"
    );
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn drawing_error_name_invalid_chars() {
    let (dir, _out) = build(json!({
        "name": "box",
        "steps": [{"op": "box", "size": [10, 10, 10]}]
    }))
    .await;
    let err = run(
        "part_drawing",
        json!({"path": "models/box.step", "name": "Box Upper"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("name may only use lowercase letters, digits, - and _"),
        "{err}"
    );
}

#[tokio::test]
#[ignore = "stage 2 not built yet"]
async fn drawing_error_part_does_not_fit() {
    let (dir, _out) = build(json!({
        "name": "huge_box",
        "steps": [{"op": "box", "size": [100000, 100000, 100000]}]
    }))
    .await;
    let err = run(
        "part_drawing",
        json!({"path": "models/huge_box.step"}),
        &dir.path,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("part does not fit on A2 at 1:50"),
        "{err}"
    );
}
