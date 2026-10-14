use forge_server::tools::pcb::{self, NativeBuild};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn file<'a>(built: &'a NativeBuild, path: &str) -> &'a str {
    &built
        .files
        .iter()
        .find(|(p, _)| p == path)
        .unwrap_or_else(|| panic!("missing file: {path}"))
        .1
}

fn err_msg(res: Result<NativeBuild, String>) -> String {
    match res {
        Ok(_) => panic!("expected Err, but build succeeded"),
        Err(e) => e,
    }
}

fn circuit(built: &NativeBuild) -> Vec<Value> {
    let (_, content) = built
        .files
        .iter()
        .find(|(p, _)| p.ends_with(".circuit.json"))
        .unwrap_or_else(|| panic!("missing .circuit.json"));
    serde_json::from_str::<Value>(content)
        .expect("valid circuit json")
        .as_array()
        .expect("circuit json array")
        .clone()
}

fn drills(text: &str) -> usize {
    text.lines().filter(|l| l.starts_with('X')).count()
}

#[derive(Debug, Clone)]
struct ApertureInfo {
    kind: String,
    args: Vec<f64>,
}

fn parse_apertures(gerber: &str) -> BTreeMap<usize, ApertureInfo> {
    let mut apertures = BTreeMap::new();
    for line in gerber.lines() {
        if let Some(rest) = line.strip_prefix("%ADD") {
            let rest = rest.trim_end_matches("*%");
            // e.g. "10C,0.1" or "11RoundRect,0.150000X..." or "12O,1.0X0.5"
            let num_end = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            let id: usize = rest[..num_end].parse().unwrap();
            let after_id = &rest[num_end..];
            if let Some((kind, args_str)) = after_id.split_once(',') {
                let args: Vec<f64> = args_str
                    .split('X')
                    .filter_map(|s| s.parse::<f64>().ok())
                    .collect();
                apertures.insert(
                    id,
                    ApertureInfo {
                        kind: kind.to_string(),
                        args,
                    },
                );
            }
        }
    }
    apertures
}

fn roundrect_dims(info: &ApertureInfo) -> (f64, f64) {
    assert_eq!(info.kind, "RoundRect");
    assert_eq!(info.args.len(), 9);
    let r = info.args[0];
    let (x0, y0) = (info.args[1], info.args[2]);
    let (x1, y1) = (info.args[3], info.args[4]);
    let (x2, y2) = (info.args[5], info.args[6]);
    let w = (x1 - x0).hypot(y1 - y0) + 2.0 * r;
    let h = (x2 - x1).hypot(y2 - y1) + 2.0 * r;
    (w, h)
}

fn parse_regions(gerber: &str) -> Vec<Vec<[f64; 2]>> {
    let mut regions = Vec::new();
    let mut in_region = false;
    let mut current_ring: Vec<[f64; 2]> = Vec::new();

    for line in gerber.lines() {
        if line == "G36*" {
            in_region = true;
            current_ring.clear();
        } else if line == "G37*" {
            if in_region && current_ring.len() >= 3 {
                if let (Some(first), Some(last)) = (current_ring.first(), current_ring.last()) {
                    if (first[0] - last[0]).abs() < 1e-6 && (first[1] - last[1]).abs() < 1e-6 {
                        current_ring.pop();
                    }
                }
                if current_ring.len() >= 3 {
                    regions.push(current_ring.clone());
                }
            }
            in_region = false;
            current_ring.clear();
        } else if in_region && line.starts_with('X') && line.contains('Y') {
            // Coordinate e.g. "X-1200000Y3400000D01*"
            let x_start = 1;
            let y_start = line.find('Y').unwrap();
            let d_start = line.find('D').unwrap();
            let x: f64 = line[x_start..y_start].parse().unwrap();
            let y: f64 = line[y_start + 1..d_start].parse().unwrap();
            current_ring.push([x / 1e6, y / 1e6]);
        }
    }
    regions
}

fn polygon_area(ring: &[[f64; 2]]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        area += ring[i][0] * ring[j][1] - ring[j][0] * ring[i][1];
    }
    (area / 2.0).abs()
}

// -----------------------------------------------------------------------------
// 1. IPC-D-356A netlist
// -----------------------------------------------------------------------------
#[test]
fn test_ipc_d_356a_netlist_format_records_nets_and_coordinates() {
    let name = "ipctest";
    let input = json!({
        "board": {"width": 30, "height": 20},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [-8, -2]},
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [8, -2], "side": "bottom"},
            {"ref": "R2", "kind": "resistor", "footprint": "0603", "at": [8, 5]}
        ],
        "nets": {
            "VERY_LONG_SIGNAL_NAME_EXCEEDING_14": ["J1.1", "R1.1"],
            "SHORT_NET": ["J1.2", "R2.1"],
            "LAYER_CROSSING": ["R1.2", "R2.2"]
        }
    });

    let built = pcb::build(&input.to_string(), name, &format!("pcb/{name}.pcb.json"))
        .expect("build succeeds");
    let ipc_path = format!("fab/{name}/{name}.ipc");
    let ipc = file(&built, &ipc_path);

    // First non-comment P lines include JOB, UNITS CUST 1, IMAGE PRIMARY
    let p_lines: Vec<&str> = ipc
        .lines()
        .filter(|l| l.starts_with("P  "))
        .collect();
    assert!(p_lines.iter().any(|l| l.contains("JOB")), "must contain JOB");
    assert!(
        p_lines.iter().any(|l| l.contains("UNITS CUST 1")),
        "must contain UNITS CUST 1"
    );
    assert!(
        p_lines.iter().any(|l| l.contains("IMAGE PRIMARY")),
        "must contain IMAGE PRIMARY"
    );

    // Ends with 999
    assert!(ipc.trim_end().ends_with("999"), "IPC must end with 999");

    // Net name longer than 14 chars produces NNAME alias record and records use alias
    let mut alias_map = BTreeMap::new();
    for line in &p_lines {
        if let Some(rest) = line.strip_prefix("P  NNAME") {
            let alias_num_end = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            let alias = format!("NNAME{}", &rest[..alias_num_end]);
            let full_name = rest[alias_num_end..].trim();
            alias_map.insert(alias, full_name.to_string());
        }
    }
    assert!(
        alias_map
            .values()
            .any(|v| v == "VERY_LONG_SIGNAL_NAME_EXCEEDING_14"),
        "expected alias for long net name"
    );

    // Compare counts against circuit JSON
    let circuit_elements = circuit(&built);
    let th_pads_count = circuit_elements
        .iter()
        .filter(|e| e["type"] == "pcb_plated_hole" || e["type"] == "pcb_hole")
        .count();
    let vias_count = circuit_elements
        .iter()
        .filter(|e| e["type"] == "pcb_via")
        .count();
    let smd_pads_count = circuit_elements
        .iter()
        .filter(|e| e["type"] == "pcb_smtpad")
        .count();

    assert!(vias_count > 0, "test setup must produce at least one via");
    assert!(th_pads_count > 0, "test setup must have through-hole pads");
    assert!(smd_pads_count > 0, "test setup must have SMD pads");

    let records_317: Vec<&str> = ipc.lines().filter(|l| l.starts_with("317")).collect();
    let records_327: Vec<&str> = ipc.lines().filter(|l| l.starts_with("327")).collect();

    assert_eq!(
        records_317.len(),
        th_pads_count + vias_count,
        "317 count must equal through-hole pads + vias"
    );
    assert_eq!(
        records_327.len(),
        smd_pads_count,
        "327 count must equal SMD pads"
    );

    // Every record's net (or NNAME alias resolved) matches circuit JSON net of that pad
    for record in records_317.iter().chain(records_327.iter()) {
        assert!(record.len() >= 70, "record length check: {record}");
        let net_raw = record[3..17].trim();
        let reference = record[20..26].trim();
        let pin = record[27..31].trim();

        let resolved_net = alias_map.get(net_raw).map(|s| s.as_str()).unwrap_or(net_raw);

        // Coordinates in mm
        let x_mm: f64 = record[41..49]
            .trim_start_matches('X')
            .parse::<f64>()
            .expect("parse x")
            / 1000.0;
        let y_mm: f64 = record[49..57]
            .trim_start_matches('Y')
            .parse::<f64>()
            .expect("parse y")
            / 1000.0;

        if reference == "VIA" {
            let matching_via = circuit_elements
                .iter()
                .filter(|e| e["type"] == "pcb_via")
                .find(|v| {
                    let vx = v["x"].as_f64().unwrap();
                    let vy = v["y"].as_f64().unwrap();
                    (vx - x_mm).hypot(vy - y_mm) < 0.001
                })
                .unwrap_or_else(|| panic!("no matching via in circuit json at [{x_mm}, {y_mm}]"));

            let via_net = matching_via["net"].as_str().unwrap_or("N/C");
            assert_eq!(resolved_net, via_net, "via net mismatch at [{x_mm}, {y_mm}]");
        } else {
            let pad_id = format!("{reference}.{pin}");
            let matching_pad = circuit_elements
                .iter()
                .find(|e| {
                    e.get("pcb_smtpad_id").and_then(|id| id.as_str()) == Some(&pad_id)
                        || e.get("pcb_plated_hole_id").and_then(|id| id.as_str()) == Some(&pad_id)
                        || e.get("pcb_hole_id").and_then(|id| id.as_str()) == Some(&pad_id)
                })
                .unwrap_or_else(|| panic!("no matching pad {pad_id} in circuit JSON"));

            let pad_net = matching_pad["net"].as_str().unwrap_or("N/C");
            assert_eq!(resolved_net, pad_net, "pad {pad_id} net mismatch");

            let px = matching_pad["x"].as_f64().unwrap();
            let py = matching_pad["y"].as_f64().unwrap();
            assert!(
                (px - x_mm).abs() <= 0.001,
                "pad {pad_id} X mismatch: circuit {px} vs IPC {x_mm}"
            );
            assert!(
                (py - y_mm).abs() <= 0.001,
                "pad {pad_id} Y mismatch: circuit {py} vs IPC {y_mm}"
            );
        }
    }
}

// -----------------------------------------------------------------------------
// 2. Stencil paste reduction and windowpane
// -----------------------------------------------------------------------------
#[test]
fn test_stencil_paste_reduction_and_windowpane_coverage() {
    let name = "pastetest";
    let base_spec = json!({
        "board": {"width": 25, "height": 25},
        "parts": [
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [-6, 0]},
            {"ref": "U1", "kind": "chip", "footprint": "qfn32", "at": [4, 0]}
        ]
    });

    // 1) Default build vs paste_reduction: 0.05
    let built_default = pcb::build(
        &base_spec.to_string(),
        name,
        &format!("pcb/{name}.pcb.json"),
    )
    .unwrap();

    let mut reduced_spec = base_spec.clone();
    reduced_spec["rules"] = json!({"paste_reduction": 0.05});
    let built_reduced = pcb::build(
        &reduced_spec.to_string(),
        name,
        &format!("pcb/{name}.pcb.json"),
    )
    .unwrap();

    // Mask Gerbers byte-identical
    for mask in ["F_Mask.gbr", "B_Mask.gbr"] {
        let p = format!("fab/{name}/{name}-{mask}");
        assert_eq!(
            file(&built_default, &p),
            file(&built_reduced, &p),
            "mask {mask} must be byte-identical"
        );
    }

    // SMD paste aperture is 0.1 mm smaller in both dims (±0.002)
    let def_paste = file(&built_default, &format!("fab/{name}/{name}-F_Paste.gbr"));
    let red_paste = file(&built_reduced, &format!("fab/{name}/{name}-F_Paste.gbr"));
    let def_apers = parse_apertures(def_paste);
    let red_apers = parse_apertures(red_paste);

    // Compare RoundRect apertures used for 0603 and QFN lead pads
    let def_roundrects: Vec<_> = def_apers
        .values()
        .filter(|a| a.kind == "RoundRect")
        .map(roundrect_dims)
        .collect();
    let red_roundrects: Vec<_> = red_apers
        .values()
        .filter(|a| a.kind == "RoundRect")
        .map(roundrect_dims)
        .collect();

    assert!(!def_roundrects.is_empty());
    assert_eq!(def_roundrects.len(), red_roundrects.len());
    for (d, r) in def_roundrects.iter().zip(red_roundrects.iter()) {
        assert!(
            ((d.0 - r.0) - 0.1).abs() < 0.002,
            "width reduction mismatch: def {} vs red {}",
            d.0,
            r.0
        );
        assert!(
            ((d.1 - r.1) - 0.1).abs() < 0.002,
            "height reduction mismatch: def {} vs red {}",
            d.1,
            r.1
        );
    }

    // 2) paste_windowpane: true on qfn32
    let mut window_spec = base_spec.clone();
    window_spec["rules"] = json!({"paste_windowpane": true});
    let built_window = pcb::build(
        &window_spec.to_string(),
        name,
        &format!("pcb/{name}.pcb.json"),
    )
    .unwrap();
    let win_paste = file(&built_window, &format!("fab/{name}/{name}-F_Paste.gbr"));

    // Exposed pad becomes >= 4 windows (G36/G37 regions)
    let regions = parse_regions(win_paste);
    assert!(
        regions.len() >= 4,
        "expected >= 4 windowpane regions, got {}",
        regions.len()
    );

    // Total area / pad area is in [0.5, 0.7] (qfn32 exposed pad is 3.1 x 3.1 mm)
    let total_window_area: f64 = regions.iter().map(|r| polygon_area(r)).sum();
    let exposed_pad_area = 3.1 * 3.1;
    let ratio = total_window_area / exposed_pad_area;
    assert!(
        (0.5..=0.7).contains(&ratio),
        "windowpane area ratio {ratio} out of bounds [0.5, 0.7]"
    );

    // Small pads unaffected: apertures with min dimension < 2.0 mm remain identical
    let win_apers = parse_apertures(win_paste);
    let def_small_apers: Vec<_> = def_apers
        .values()
        .filter(|a| a.kind == "RoundRect")
        .map(roundrect_dims)
        .filter(|(w, h)| w.min(*h) < 2.0)
        .collect();
    let win_small_apers: Vec<_> = win_apers
        .values()
        .filter(|a| a.kind == "RoundRect")
        .map(roundrect_dims)
        .filter(|(w, h)| w.min(*h) < 2.0)
        .collect();
    assert_eq!(def_small_apers, win_small_apers);

    // 3) paste_reduction: -1 errors
    let mut err_spec = base_spec.clone();
    err_spec["rules"] = json!({"paste_reduction": -1});
    let err = err_msg(pcb::build(&err_spec.to_string(), name, &format!("pcb/{name}.pcb.json")));
    assert!(
        err.contains("rules.paste_reduction must be nonnegative"),
        "error message: {err}"
    );
}

// -----------------------------------------------------------------------------
// 3. Fabrication drawing
// -----------------------------------------------------------------------------
#[test]
fn test_fab_drawing_svg_dimensions_drills_and_stackup() {
    let name = "fabtest";
    let spec = json!({
        "board": {"width": 32.5, "height": 18.5, "thickness": 1.6},
        "rules": {"via_in_pad": true},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [-5, 0]},
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [5, 0]}
        ],
        "holes": [{"at": [0, 4], "d": 2.5}]
    });

    let built = pcb::build(&spec.to_string(), name, &format!("pcb/{name}.pcb.json")).unwrap();
    let fab_path = format!("fab/{name}/{name}-fab.svg");
    let svg = file(&built, &fab_path);

    // Parses as XML-ish (starts with <svg, balanced root)
    assert!(svg.starts_with("<svg"), "must start with <svg");
    assert!(svg.trim_end().ends_with("</svg>"), "must end with </svg>");
    assert_eq!(
        svg.matches("<svg").count(),
        1,
        "root <svg> count must be 1"
    );
    assert_eq!(
        svg.matches("</svg>").count(),
        1,
        "closing </svg> count must be 1"
    );

    // Contains board width and height values (2 decimals)
    assert!(
        svg.contains("32.50 mm"),
        "must contain width formatted with 2 decimals"
    );
    assert!(
        svg.contains("18.50 mm"),
        "must contain height formatted with 2 decimals"
    );

    // Drill table whose per-tool counts sum to total holes in PTH+NPTH drill files
    let drill_table_count: usize = svg
        .split("data-count=\"")
        .skip(1)
        .map(|s| s.split('"').next().unwrap().parse::<usize>().unwrap())
        .sum();

    let pth = file(&built, &format!("fab/{name}/{name}-PTH.drl"));
    let npth = file(&built, &format!("fab/{name}/{name}-NPTH.drl"));
    let total_drill_hits = drills(pth) + drills(npth);

    assert_eq!(
        drill_table_count, total_drill_hits,
        "drill table sum must match PTH + NPTH hits"
    );

    // Stackup thickness text summing to board thickness
    let stackup_thickness_sum: f64 = svg
        .split("data-thickness=\"")
        .skip(1)
        .map(|s| s.split('"').next().unwrap().parse::<f64>().unwrap())
        .sum();
    assert!(
        (stackup_thickness_sum - 1.6).abs() < 0.001,
        "stackup thickness sum {stackup_thickness_sum} must equal board thickness 1.6"
    );
    assert!(svg.contains("Total thickness: 1.6 mm"));

    // Contains 'Filled/capped vias-in-pad are required.' only when via_in_pad is true
    assert!(
        svg.contains("Filled/capped vias-in-pad are required."),
        "must contain note when via_in_pad: true"
    );

    let mut spec_no_via_in_pad = spec.clone();
    spec_no_via_in_pad["rules"] = json!({"via_in_pad": false});
    let built_no_vip = pcb::build(
        &spec_no_via_in_pad.to_string(),
        name,
        &format!("pcb/{name}.pcb.json"),
    )
    .unwrap();
    let svg_no_vip = file(&built_no_vip, &fab_path);
    assert!(
        !svg_no_vip.contains("Filled/capped vias-in-pad are required."),
        "must NOT contain note when via_in_pad: false"
    );
}

// -----------------------------------------------------------------------------
// 4. Slots in 3D export
// -----------------------------------------------------------------------------
#[test]
fn test_slots_3d_step_glb_and_drill_records() {
    let name = "slottest";
    let spec = json!({
        "board": {"width": 30, "height": 20},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "usb_c_16", "at": [0, 0]}
        ]
    });

    let built = pcb::build(&spec.to_string(), name, &format!("pcb/{name}.pcb.json"))
        .expect("build with usb_c_16 succeeds");

    // STEP contains no error
    let step = file(&built, &format!("pcb/{name}.step"));
    assert!(step.contains("ISO-10303-21"), "valid STEP header");
    assert!(!step.to_lowercase().contains("error"), "STEP has no error");

    // GLB is non-empty
    let (_, glb) = built
        .binary_files
        .iter()
        .find(|(p, _)| p == &format!("pcb/{name}.glb"))
        .expect("missing glb binary file");
    assert!(!glb.is_empty(), "GLB must be non-empty");
    assert!(glb.len() > 100, "GLB has substantial content");

    // NPTH/PTH drill files contain G85 slot records
    let pth = file(&built, &format!("fab/{name}/{name}-PTH.drl"));
    let npth = file(&built, &format!("fab/{name}/{name}-NPTH.drl"));
    let g85_count = pth.matches("G85").count() + npth.matches("G85").count();
    assert!(
        g85_count > 0,
        "drill files must contain G85 slot records for usb_c_16"
    );
    assert_eq!(g85_count, 4, "usb_c_16 has 4 slot holes");
}

// -----------------------------------------------------------------------------
// 5. Panel tabs 2x3
// -----------------------------------------------------------------------------
#[test]
fn test_panel_tabs_2x3_layers_flashes_drills_and_pnp() {
    let name = "paneltest";
    let single_spec = json!({
        "board": {"width": 20, "height": 15},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [-5, -2]},
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [5, 2], "side": "bottom"}
        ],
        "holes": [{"at": [0, 5], "d": 1}],
        "nets": {"N": ["J1.1", "R1.1"]}
    });
    let single = pcb::build(
        &single_spec.to_string(),
        name,
        &format!("pcb/{name}.pcb.json"),
    )
    .unwrap();

    let mut panel_spec = single_spec.clone();
    panel_spec["panel"] = json!({"rows": 2, "cols": 3});
    let built = pcb::build(
        &panel_spec.to_string(),
        name,
        &format!("pcb/{name}.pcb.json"),
    )
    .unwrap();

    // Report line: Panel: 2 x 3 boards, 64 x 42 mm, tabs.
    // W = cols*bw + (cols-1)*spacing = 3*20 + 2*2 = 64
    // H = rows*bh + (rows-1)*spacing + 2*rails = 2*15 + 1*2 + 2*5 = 42
    assert!(
        built
            .report
            .contains("Panel: 2 x 3 boards, 64 x 42 mm, tabs."),
        "report mismatch: {}",
        built.report
    );

    // Panel folder files exist for every single-board layer
    for layer in [
        "F_Cu.gbr",
        "B_Cu.gbr",
        "F_Mask.gbr",
        "B_Mask.gbr",
        "F_Paste.gbr",
        "B_Paste.gbr",
        "F_Silkscreen.gbr",
        "B_Silkscreen.gbr",
        "Edge_Cuts.gbr",
        "PTH.drl",
        "NPTH.drl",
    ] {
        let single_file = format!("fab/{name}/{name}-{layer}");
        let panel_file = format!("fab/{name}-panel/{name}-panel-{layer}");
        assert!(
            built.files.iter().any(|(p, _)| p == &single_file),
            "missing single file {single_file}"
        );
        assert!(
            built.files.iter().any(|(p, _)| p == &panel_file),
            "missing panel file {panel_file}"
        );
    }

    // Panel F_Cu flash count = 6 x single F_Cu flash count + 3 fiducials
    let single_f_cu = file(&single, &format!("fab/{name}/{name}-F_Cu.gbr"));
    let panel_f_cu = file(&built, &format!("fab/{name}-panel/{name}-panel-F_Cu.gbr"));
    assert_eq!(
        panel_f_cu.matches("D03*").count(),
        6 * single_f_cu.matches("D03*").count() + 3
    );

    // Panel PTH hit count = 6 x single PTH hits
    let single_pth = file(&single, &format!("fab/{name}/{name}-PTH.drl"));
    let panel_pth = file(&built, &format!("fab/{name}-panel/{name}-panel-PTH.drl"));
    assert_eq!(drills(panel_pth), 6 * drills(single_pth));

    // Panel NPTH contains tooling holes (Ø2.0) x 3 and mouse-bite holes (Ø0.5)
    let panel_npth = file(&built, &format!("fab/{name}-panel/{name}-panel-NPTH.drl"));
    assert!(
        panel_npth.contains("C0.5\n"),
        "NPTH must contain Ø0.5 mousebites"
    );
    assert!(
        panel_npth.contains("C2\n") || panel_npth.contains("C2.0\n"),
        "NPTH must contain Ø2.0 tooling"
    );

    // Edge_Cuts bounding box equals panel size (64 x 42)
    let edge = file(
        &built,
        &format!("fab/{name}-panel/{name}-panel-Edge_Cuts.gbr"),
    );
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for line in edge.lines() {
        if line.starts_with('X') && line.contains('Y') && line.contains('D') {
            let y_pos = line.find('Y').unwrap();
            let d_pos = line.find('D').unwrap();
            let x: f64 = line[1..y_pos].parse().unwrap();
            let y: f64 = line[y_pos + 1..d_pos].parse().unwrap();
            let x_mm = x / 1e6;
            let y_mm = y / 1e6;
            min_x = min_x.min(x_mm);
            max_x = max_x.max(x_mm);
            min_y = min_y.min(y_mm);
            max_y = max_y.max(y_mm);
        }
    }
    assert!(min_x.abs() < 0.001, "min X {min_x} != 0");
    assert!((max_x - 64.0).abs() < 0.001, "max X {max_x} != 64");
    assert!(min_y.abs() < 0.001, "min Y {min_y} != 0");
    assert!((max_y - 42.0).abs() < 0.001, "max Y {max_y} != 42");

    // Panel PnP has 6 x single rows with _r1c1 ... _r2c3 suffixes
    let pnp = file(&built, &format!("fab/{name}-panel/{name}-panel-pnp.csv"));
    let pnp_rows: Vec<&str> = pnp.lines().collect();
    // 1 header + 6 * 2 parts = 13 lines
    assert_eq!(pnp_rows.len(), 1 + 6 * 2);
    for row in 1..=2 {
        for col in 1..=3 {
            let suffix = format!("_r{row}c{col}");
            assert!(
                pnp.contains(&suffix),
                "PnP missing designator suffix {suffix}"
            );
        }
    }
}

// -----------------------------------------------------------------------------
// 6. Panel V-score
// -----------------------------------------------------------------------------
#[test]
fn test_panel_vscore_separations_score_lines_and_no_mousebites() {
    let name = "vscoretest";
    let spec = json!({
        "board": {"width": 20, "height": 15},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [-5, -2]},
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [5, 2], "side": "bottom"}
        ],
        "holes": [{"at": [0, 5], "d": 1}],
        "panel": {"rows": 2, "cols": 3, "separation": "vscore"}
    });

    let built = pcb::build(&spec.to_string(), name, &format!("pcb/{name}.pcb.json")).unwrap();

    // Spacing 0 default, rails 5 on y: W = 3*20 = 60, H = 2*15 + 10 = 40
    assert!(built
        .report
        .contains("Panel: 2 x 3 boards, 60 x 40 mm, vscore."));

    // -VScore.gbr exists and has %TF.FileFunction,Other,VScore*%
    let vscore = file(
        &built,
        &format!("fab/{name}-panel/{name}-panel-VScore.gbr"),
    );
    assert!(vscore.contains("%TF.FileFunction,Other,VScore*%"));

    // Score lines spanning the panel:
    // (cols - 1) = 2 interior vertical lines at X=20 and X=40 spanning Y=0 to Y=40
    // (rows - 1) = 1 interior horizontal line at Y=20, plus 2 rail boundaries at Y=5 and Y=35 spanning X=0 to X=60
    // Total 5 score lines
    assert_eq!(vscore.matches("D01*").count(), 5);
    assert!(vscore.contains("X20000000Y0D02*\nX20000000Y40000000D01*\n"));
    assert!(vscore.contains("X40000000Y0D02*\nX40000000Y40000000D01*\n"));
    assert!(vscore.contains("X0Y5000000D02*\nX60000000Y5000000D01*\n"));
    assert!(vscore.contains("X0Y20000000D02*\nX60000000Y20000000D01*\n"));
    assert!(vscore.contains("X0Y35000000D02*\nX60000000Y35000000D01*\n"));

    // No mouse-bite holes in NPTH
    let npth = file(&built, &format!("fab/{name}-panel/{name}-panel-NPTH.drl"));
    assert!(
        !npth.contains("C0.5\n"),
        "vscore panel must have NO mouse-bite holes"
    );
}

// -----------------------------------------------------------------------------
// 7. Validation errors
// -----------------------------------------------------------------------------
#[test]
fn test_panel_and_paste_validation_errors() {
    let base = json!({
        "board": {"width": 20, "height": 15},
        "panel": {"rows": 2, "cols": 3}
    });

    for (field, value, expected_err) in [
        ("rows", json!(0), "panel.rows must be 1..20"),
        ("rows", json!(21), "panel.rows must be 1..20"),
        ("rows", json!(-1), "panel.rows must be 1..20"),
        ("cols", json!(0), "panel.cols must be 1..20"),
        ("cols", json!(21), "panel.cols must be 1..20"),
        ("cols", json!(-1), "panel.cols must be 1..20"),
        ("spacing", json!(0.5), "panel: tabs need spacing of at least 1 mm"),
        ("spacing", json!(-1), "panel.spacing must be nonnegative"),
        ("rails", json!(-1), "panel.rails must be nonnegative"),
        ("rails", json!(0), "panel: fiducials and tooling holes need rails"),
        ("tab_width", json!(0), "panel.tab_width must be positive"),
        ("tab_width", json!(-1), "panel.tab_width must be positive"),
        ("tabs", json!(0), "panel.tabs must be positive"),
        ("tabs", json!(65), "panel.tabs must be at most 64"),
        ("separation", json!("laser"), "panel.separation must be tabs or vscore"),
        ("rail_sides", json!("z"), "panel.rail_sides must be x, y or both"),
    ] {
        let mut input = base.clone();
        input["panel"][field] = value;
        let err = pcb::parse(&input.to_string()).unwrap_err();
        assert_eq!(err, expected_err, "failed asserting error for {field}");
    }

    // Non-rectangular outline errors for tabs and vscore
    let non_rect = json!({
        "board": {"outline": [[-10, -8], [10, -8], [0, 8]]},
        "panel": {"rows": 2, "cols": 3, "separation": "tabs"}
    });
    assert_eq!(
        pcb::parse(&non_rect.to_string()).unwrap_err(),
        "panel: tabs need a rectangular board"
    );

    let mut non_rect_vscore = non_rect.clone();
    non_rect_vscore["panel"]["separation"] = json!("vscore");
    assert_eq!(
        pcb::parse(&non_rect_vscore.to_string()).unwrap_err(),
        "panel: vscore needs a rectangular board"
    );

    // vscore with non-zero spacing
    let mut vscore_spacing = base.clone();
    vscore_spacing["panel"]["separation"] = json!("vscore");
    vscore_spacing["panel"]["spacing"] = json!(1.0);
    assert_eq!(
        pcb::parse(&vscore_spacing.to_string()).unwrap_err(),
        "panel: vscore needs spacing of 0 mm"
    );

    // paste_reduction negative error
    let mut paste_err = base.clone();
    paste_err["rules"] = json!({"paste_reduction": -0.05});
    assert_eq!(
        pcb::parse(&paste_err.to_string()).unwrap_err(),
        "rules.paste_reduction must be nonnegative"
    );
}

// -----------------------------------------------------------------------------
// 8. Defaults
// -----------------------------------------------------------------------------
#[test]
fn test_defaults_no_panel_files_and_paste_defaults() {
    let name = "deftest";
    let spec = json!({
        "board": {"width": 20, "height": 15},
        "parts": [
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [0, 0]}
        ]
    });

    let built = pcb::build(&spec.to_string(), name, &format!("pcb/{name}.pcb.json")).unwrap();

    // Without panel: no -panel files and report has no Panel:
    assert!(
        !built.report.contains("Panel:"),
        "report must not mention Panel"
    );
    for (path, _) in &built.files {
        assert!(
            !path.contains("-panel"),
            "unexpected panel file emitted: {path}"
        );
    }

    // Without paste fields, paste Gerbers equal explicit paste_reduction: 0, paste_windowpane: false
    let mut explicit_spec = spec.clone();
    explicit_spec["rules"] = json!({
        "paste_reduction": 0,
        "paste_windowpane": false
    });
    let built_explicit = pcb::build(
        &explicit_spec.to_string(),
        name,
        &format!("pcb/{name}.pcb.json"),
    )
    .unwrap();

    assert_eq!(
        file(&built, &format!("fab/{name}/{name}-F_Paste.gbr")),
        file(&built_explicit, &format!("fab/{name}/{name}-F_Paste.gbr")),
        "default F_Paste must match explicit defaults"
    );
    assert_eq!(
        file(&built, &format!("fab/{name}/{name}-B_Paste.gbr")),
        file(&built_explicit, &format!("fab/{name}/{name}-B_Paste.gbr")),
        "default B_Paste must match explicit defaults"
    );
}

// -----------------------------------------------------------------------------
// 9. Determinism
// -----------------------------------------------------------------------------
#[test]
fn test_panel_determinism_repeated_build() {
    let name = "determtest";
    let spec = json!({
        "board": {"width": 20, "height": 15},
        "parts": [
            {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [-5, -2]},
            {"ref": "R1", "kind": "resistor", "footprint": "0603", "at": [5, 2], "side": "bottom"}
        ],
        "holes": [{"at": [0, 5], "d": 1}],
        "panel": {"rows": 2, "cols": 3}
    });

    let build1 = pcb::build(&spec.to_string(), name, &format!("pcb/{name}.pcb.json")).unwrap();
    let build2 = pcb::build(&spec.to_string(), name, &format!("pcb/{name}.pcb.json")).unwrap();

    assert_eq!(build1.report, build2.report, "report must be deterministic");
    assert_eq!(build1.files, build2.files, "text files must be byte-identical");
    assert_eq!(
        build1.binary_files, build2.binary_files,
        "binary files must be byte-identical"
    );
}
