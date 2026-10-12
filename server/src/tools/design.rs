use super::{fit::{apply, board_bounds, fit_check, parse_stl, rotation_matrix, Placement, FitBoard, FitSolid, Vec3}, paths::project_path, script3d::valid_name, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::jsfmt::{json_number, number, numeric, round, string, stringify_pretty};
use serde_json::{json, Value};

pub fn tools() -> Vec<Tool> {
    vec![
        {
            Tool::new("pcb", "design_assemble", r#"Put circuit boards and mechanical parts together in one 3D assembly, like an electronics + mechanical design in Fusion 360, and check that the boards fit. Writes design/<name>.json, which the Design view shows: the board's PCB layout beside the 3D assembly.
Parts are board models from pcb_build (pcb/<name>.glb) and enclosure parts from freecad_build (.stl, or the .step with its .stl beside it), each placed with position [x, y, z] in mm and rotation [rx, ry, rz] in degrees. A board's origin is its centre, at half its thickness. FreeCAD parts keep their own coordinates.
The fit check tests the board and every component's 3D box against every mechanical part: a collision means a part cuts into the enclosure (move the board, the part or the enclosure feature); tight means closer than the clearance. It also reports the space each board takes up, to size or adjust an enclosure around it. Re-run it after changing either side."#, json!({"type":"object","properties":{"name":{"type":"string","description":"Assembly name: letters, digits, - and _"},"parts":{"type":"array","minItems":1,"items":{"type":"object","properties":{"file":{"type":"string","description":"Project-relative .glb (board) or .stl/.step (mechanical)"},"position":{"type":"array","items":{"type":"number"},"description":"[x, y, z] in mm (default 0, 0, 0)"},"rotation":{"type":"array","items":{"type":"number"},"description":"[rx, ry, rz] in degrees (default 0, 0, 0)"},"color":{"type":"string","description":"Display colour such as #b0b8c8 (mechanical parts)"},"opacity":{"type":"number","description":"Display opacity, e.g. 0.35 to see a board through its case (default 1)"}},"required":["file"]}},"clearance":{"type":"number","description":"Gap in mm closer than which parts are reported as tight (default 0.5)"}},"required":["name","parts"]}), design_assemble)
        },
    ]
}

fn vec(value: Option<&Value>, what: &str) -> Result<Value, ToolError> {
    let Some(value) = value else { return Ok(json!([0, 0, 0])); };
    if !value.as_array().is_some_and(|v| v.len() == 3 && v.iter().all(|n| n.is_number() && n.as_f64().is_some_and(f64::is_finite))) {
        return Err(ToolError::expected(format!("{what} must be three numbers")));
    }
    Ok(value.clone())
}

async fn exists(ctx: &ToolContext, file: &str) -> Result<bool, ToolError> {
    Ok(tokio::fs::metadata(project_path(&ctx.project_dir, file)?).await.is_ok_and(|m| m.is_file()))
}

/// The board data next to a board's 3D model: pcb/main.glb -> pcb/main.board.json.
pub fn board_data_for(file: &str) -> Option<String> {
    file.to_lowercase().ends_with(".glb").then(|| format!("{}.board.json", &file[..file.len() - 4]))
}

async fn check_part(ctx: &ToolContext, raw: &Value) -> Result<Value, ToolError> {
    let mut file = string(Some(raw.get("file").filter(|v| !v.is_null()).unwrap_or(&Value::String(String::new()))));
    // A STEP or FreeCAD file is shown and checked through the mesh freecad_build writes next to it.
    let ext = file.rsplit('.').next().unwrap_or("").to_lowercase();
    if ["step", "stp", "fcstd"].contains(&ext.as_str()) { file = format!("{}.stl", file.rsplit_once('.').unwrap().0); }
    if !["stl", "glb"].iter().any(|ext| file.to_lowercase().ends_with(&format!(".{ext}"))) {
        return Err(ToolError::expected(format!("{}: parts must be .stl or .glb models (or a .step with its .stl beside it)", string(raw.get("file")))));
    }
    if !exists(ctx, &file).await? { return Err(ToolError::expected(format!("{file} does not exist; build it first (pcb_build for a board, freecad_build for an enclosure)"))); }
    let mut part = json!({"file":file,"position":vec(raw.get("position"), &format!("{file} position"))?,"rotation":vec(raw.get("rotation"), &format!("{file} rotation"))?});
    if let Some(color) = raw.get("color") {
        if !color.as_str().is_some_and(|s| s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())) {
            return Err(ToolError::expected(format!("{file} color must look like #8899aa")));
        }
        part["color"] = color.clone();
    }
    if let Some(opacity) = raw.get("opacity") {
        let o = numeric(Some(opacity));
        if !(o > 0.0 && o <= 1.0) { return Err(ToolError::expected(format!("{file} opacity must be above 0 and at most 1"))); }
        part["opacity"] = json_number(o);
    }
    Ok(part)
}

fn fmt(values: &[f64]) -> String {
    values.iter().map(|n| number(round(n * 100.0) / 100.0)).collect::<Vec<_>>().join(", ")
}

fn placement(part: &Value) -> Placement {
    let read = |key: &str| -> Vec3 { [0, 1, 2].map(|i| part[key][i].as_f64().unwrap()) };
    Placement { position: read("position"), rotation: read("rotation") }
}

fn design_assemble(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let name = string(Some(input.get("name").filter(|v| !v.is_null()).unwrap_or(&json!(""))));
        if !valid_name(&name) { return Err(ToolError::expected("name may only use letters, digits, - and _")); }
        let raw = input["parts"].as_array().filter(|a| !a.is_empty() && a.len() <= 50).ok_or_else(|| ToolError::expected("give 1 to 50 parts"))?;
        let clearance = input.get("clearance").map_or(0.5, |v| numeric(Some(v)));
        if !(0.0..=20.0).contains(&clearance) { return Err(ToolError::expected("clearance must be between 0 and 20 mm")); }
        let mut parts = Vec::new();
        for p in raw { parts.push(check_part(&ctx, p).await?); }
        let (mut boards, mut solids, mut shown_only) = (Vec::new(), Vec::new(), Vec::new());
        for p in &parts {
            let file = p["file"].as_str().unwrap();
            let data = board_data_for(file);
            if let Some(data) = data.filter(|data| project_path(&ctx.project_dir, data).is_ok_and(|p| p.is_file())) {
                boards.push(FitBoard { name: file.into(), placement: placement(p), board: serde_json::from_slice(&tokio::fs::read(project_path(&ctx.project_dir, &data)?).await?)? });
            } else if file.to_lowercase().ends_with(".stl") {
                let triangles = async {
                    let data = tokio::fs::read(project_path(&ctx.project_dir, file)?).await?;
                    parse_stl(&data)
                }.await.map_err(|e| ToolError::expected(format!("{file}: {e}")))?;
                solids.push(FitSolid { name: file.into(), placement: placement(p), triangles });
            } else { shown_only.push(file.to_owned()); }
        }
        let fit = fit_check(&boards, &solids, clearance);
        // What design/<name>.json holds: the parts the Design view shows together, and the last fit check.
        let design = json!({"parts":parts,"clearance":clearance,"fit":fit});
        let rel = format!("design/{name}.json");
        let full = project_path(&ctx.project_dir, &rel)?;
        tokio::fs::create_dir_all(project_path(&ctx.project_dir, "design")?).await?;
        tokio::fs::write(full, stringify_pretty(&design)? + "\n").await?;
        let mut lines = vec![format!("Assembly {rel}: {} part(s), {} board(s).", parts.len(), boards.len())];
        for b in &boards {
            let (min, max) = board_bounds(&b.board, &b.placement);
            let mut components = b.board.components.iter().collect::<Vec<_>>();
            components.sort_by(|x, y| y.max[2].partial_cmp(&x.max[2]).unwrap_or(std::cmp::Ordering::Equal));
            let tallest = components.first().map_or(String::new(), |c| format!("; tallest part {} reaches {} mm above the board", c.name, fmt(&[c.max[2] - b.board.size[2] / 2.0])));
            lines.push(format!("  {} with its parts spans x {}, y {}, z {} mm{tallest}", b.name, fmt(&[min[0], max[0]]), fmt(&[min[1], max[1]]), fmt(&[min[2], max[2]])));
        }
        for s in &solids {
            let m = rotation_matrix(s.placement.rotation);
            let (mut min, mut max) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for t in s.triangles.chunks_exact(3) {
                let p = apply(m, s.placement.position, [t[0] as f64, t[1] as f64, t[2] as f64]);
                for k in 0..3 { min[k] = min[k].min(p[k]); max[k] = max[k].max(p[k]); }
            }
            lines.push(format!("  {} spans x {}, y {}, z {} mm", s.name, fmt(&[min[0], max[0]]), fmt(&[min[1], max[1]]), fmt(&[min[2], max[2]])));
        }
        if !shown_only.is_empty() { lines.push(format!("  Shown but not checked (not an STL or a board): {}", shown_only.join(", "))); }
        if boards.is_empty() || solids.is_empty() { lines.push("Fit check: needs at least one board and one mechanical part.".into()); }
        else {
            let collisions: Vec<_> = fit.iter().filter(|f| f.status == "collision").collect();
            let tight: Vec<_> = fit.iter().filter(|f| f.status == "tight").collect();
            if fit.is_empty() { lines.push(format!("Fit check: everything clears by at least {} mm.", number(clearance))); }
            if !collisions.is_empty() {
                lines.push(format!("Fit check: {} COLLISION(S):", collisions.len()));
                lines.extend(collisions.iter().map(|f| format!("  {} on {} cuts into {}", if f.item == "board" { "the board" } else { &f.item }, f.board, f.part)));
            }
            if !tight.is_empty() {
                lines.push(format!("Within {} mm (tight, check it is intended, e.g. the board resting on standoffs):", number(clearance)));
                lines.extend(tight.iter().map(|f| format!("  {} on {} and {}", if f.item == "board" { "the board" } else { &f.item }, f.board, f.part)));
            }
        }
        Ok(ToolResult { text: lines.join("\n"), files: vec![rel] })
    })
}
