use super::{exec::{clip, find_program, run}, paths::project_path, script3d::{files, joined, parse_runner_output, run_app, valid_name, FREECAD}, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::jsfmt::{numeric, string, truthy};
use serde_json::{json, Value};

// Keep in step with MATERIALS in runners/forge_cad.py.
pub const MATERIALS: &[&str] = &["PLA", "PETG", "ABS", "ASA", "Nylon PA12", "Polycarbonate", "Acrylic", "POM (Delrin)", "Aluminium 6061", "Aluminium 7075", "Steel", "Stainless 304", "Brass", "Copper", "Titanium Ti-6Al-4V", "Plywood"];

pub fn tools() -> Vec<Tool> {
    vec![
        {
            let mut tool = Tool::new("3d", "freecad_inspect", r#"Measure FreeCAD parts exactly: validity, volume, mass in a chosen material, bounding box, centre of mass, every hole (diameter, through or depth, counterbore/countersink, which screw it fits), 3D-print overhang area for the current orientation, and interference or clearance between parts (each solid in each file is a part, so an assembly can be checked). Use it to verify a design against the requirements before calling it done, and to check that parts fit together."#, json!({"type":"object", "additionalProperties": false,"properties":{"models":{"type":"array","items":{"type":"string"},"minItems":1,"description":"Project-relative .step/.FCStd files"},"material":{"type":"string","enum":["PLA","PETG","ABS","ASA","Nylon PA12","Polycarbonate","Acrylic","POM (Delrin)","Aluminium 6061","Aluminium 7075","Steel","Stainless 304","Brass","Copper","Titanium Ti-6Al-4V","Plywood"],"description":"Material for the mass"},"overhang":{"type":"number","description":"Steepest printable overhang from vertical in degrees (default 45)"}},"required":["models"]}), freecad_inspect);
            tool.app = Some(FREECAD.clone());
            tool.install = Some(serde_json::from_value(json!({"flatpak":"org.freecad.FreeCAD","apt":"freecad","brew":"--cask freecad","url":"https://www.freecad.org/downloads.php"})).unwrap());
            tool
        },
        {
            let mut tool = Tool::new("3d", "freecad_drawing", r#"Make a technical drawing of a FreeCAD part for a workshop or manufacturer: front, top and side views with hidden lines, an isometric view, overall dimensions, every hole tagged on the views and listed in a hole table (size, through/depth, counterbore, position from the part's corner, screw it fits), and a title block with title, material, mass, scale, sheet and projection symbol (general tolerance ISO 2768-m). Writes drawings/<name>.svg, drawings/<name>.pdf when rsvg-convert is installed, and with dxf=true drawings/<name>-top.dxf (the top outline at 1:1 with true arcs, for laser cutting or CNC)."#, json!({"type":"object", "additionalProperties": false,"properties":{"model":{"type":"string","description":"Project-relative .step or .FCStd file"},"name":{"type":"string","description":"Base name for the drawing files: letters, digits, - and _"},"title":{"type":"string","description":"Title in the title block (default: the name)"},"material":{"type":"string","enum":["PLA","PETG","ABS","ASA","Nylon PA12","Polycarbonate","Acrylic","POM (Delrin)","Aluminium 6061","Aluminium 7075","Steel","Stainless 304","Brass","Copper","Titanium Ti-6Al-4V","Plywood"],"description":"Material for the mass and drawing title block"},"projection":{"type":"string","enum":["first","third"],"description":"first angle (ISO, Europe, India; default) or third angle (USA)"},"sheet":{"type":"string","enum":["auto","A4","A3","A2"],"description":"Sheet size (default: smallest that fits at 1:1 or larger)"},"dxf":{"type":"boolean","description":"Also export the top view as DXF"}},"required":["model","name"]}), freecad_drawing);
            tool.app = Some(FREECAD.clone());
            tool.install = Some(serde_json::from_value(json!({"flatpak":"org.freecad.FreeCAD","apt":"freecad","brew":"--cask freecad","url":"https://www.freecad.org/downloads.php"})).unwrap());
            tool
        },
    ]
}

async fn check_model(ctx: &ToolContext, rel: &Value) -> Result<String, ToolError> {
    let rel = rel.as_str().ok_or_else(|| ToolError::expected("model paths must be strings"))?;
    let full = project_path(&ctx.project_dir, rel)?;
    let ext = full.extension().unwrap_or_default().to_string_lossy().to_lowercase();
    if !["step", "stp", "brep", "fcstd", "iges", "igs"].contains(&ext.as_str()) {
        return Err(ToolError::expected(format!("{rel}: use a STEP, BREP, IGES or FCStd file (an STL mesh has no exact geometry)")));
    }
    if !tokio::fs::metadata(full).await.is_ok_and(|m| m.is_file()) {
        return Err(ToolError::expected(format!("{rel} does not exist; build it with freecad_build first")));
    }
    Ok(rel.into())
}

fn check_material(value: Option<&Value>) -> Result<Option<&'static str>, ToolError> {
    let Some(value) = value else { return Ok(None); };
    let material = string(Some(value));
    MATERIALS.iter().find(|m| m.to_lowercase() == material.to_lowercase()).copied().map(Some)
        .ok_or_else(|| ToolError::expected(format!("unknown material {material}; use one of: {}", MATERIALS.join(", "))))
}

async fn run_freecad(runner: &str, ctx: &ToolContext, args: Value) -> Result<Value, ToolError> {
    let r = run_app(&FREECAD, vec![ctx.app_dir.join("server/runners").join(runner).to_string_lossy().into_owned()], ctx, 5 * 60_000, vec![
        ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
        ("FORGE_PROJECT".into(), project_path(&ctx.project_dir, "")?.to_string_lossy().into_owned()),
        ("FORGE_ARGS".into(), serde_json::to_string(&args)?),
    ]).await?;
    let parsed = parse_runner_output(&r)?;
    if let Some(error) = parsed.error { return Err(ToolError::expected(clip(&format!("FreeCAD: {error}"), 12_000))); }
    Ok(parsed.result.unwrap())
}

fn hole_line(h: &Value) -> String {
    let depth = if truthy(&h["through"]) { "through".into() } else { format!("{} deep", string(h.get("depth"))) };
    let mut s = format!("Ø{} {depth} along {} at ({})", string(h.get("diameter")), string(h.get("axis")), joined(&h["center"], ", "));
    if truthy(&h["counterbore"]) { s.push_str(&format!(", counterbore Ø{} × {}", string(h["counterbore"].get(0)), string(h["counterbore"].get(1)))); }
    if truthy(&h["countersink"]) { s.push_str(&format!(", countersink Ø{}", string(h.get("countersink")))); }
    if truthy(&h["fits"]) { s.push_str(&format!(" [{}]", string(h.get("fits")))); }
    s
}

fn freecad_inspect(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let models = input["models"].as_array().filter(|m| !m.is_empty() && m.len() <= 20).ok_or_else(|| ToolError::expected("give 1 to 20 models"))?;
        for m in models { check_model(&ctx, m).await?; }
        let material = check_material(input.get("material"))?;
        let mut args = json!({"models":models});
        if let Some(material) = material { args["material"] = json!(material); }
        args["overhang"] = input.get("overhang").filter(|v| !v.is_null()).cloned().unwrap_or(json!(45));
        let res = run_freecad("freecad_inspect.py", &ctx, args).await?;
        let mut lines = Vec::new();
        for p in res["parts"].as_array().unwrap() {
            lines.push(format!("{}: {} solid(s), valid: {}{}", string(p.get("file")), string(p.get("solids")), string(p.get("valid")), if truthy(&p["valid"]) { "" } else { " — fix the geometry before manufacturing" }));
            lines.push(format!("  size {} mm from ({}), volume {} mm³, area {} mm²", joined(&p["size_mm"], " × "), joined(&p["min_mm"], ", "), string(p.get("volume_mm3")), string(p.get("area_mm2"))));
            if p.get("mass_g").is_some() { lines.push(format!("  mass {} g in {} ({} g/cm³)", string(p.get("mass_g")), string(res.get("material")), string(res.get("density_g_cm3")))); }
            if truthy(&p["center_of_mass_mm"]) { lines.push(format!("  centre of mass ({})", joined(&p["center_of_mass_mm"], ", "))); }
            let holes = p["holes"].as_array().unwrap();
            lines.push(if holes.is_empty() { "  no holes".into() } else { format!("  {} hole(s):", holes.len()) });
            for h in holes.iter().take(40) { lines.push(format!("    {}", hole_line(h))); }
            if holes.len() > 40 { lines.push(format!("    … and {} more", holes.len() - 40)); }
            if p.get("overhang_mm2").is_some() {
                lines.push(format!("  printing as oriented: {} mm² overhangs steeper than {}° ({}% of the surface), {} mm² on the bed", string(p.get("overhang_mm2")), string(res.get("overhang_angle")), string(p.get("overhang_percent")), string(p.get("bed_contact_mm2"))));
            }
        }
        let pairs = res["pairs"].as_array().unwrap();
        if !pairs.is_empty() {
            lines.push("Between parts (pairs within 5 mm):".into());
            for pr in pairs {
                let detail = if pr.get("interference_mm3").is_some() { format!("INTERFERE by {} mm³", string(pr.get("interference_mm3"))) }
                    else { format!("clearance {} mm{}", string(pr.get("clearance_mm")), if numeric(pr.get("clearance_mm")) < 0.2 { " (touching or very tight)" } else { "" }) };
                lines.push(format!("  {} and {}: {detail}", string(pr.get("a")), string(pr.get("b"))));
            }
        }
        Ok(ToolResult::text(lines.join("\n")))
    })
}

fn freecad_drawing(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let name = string(input.get("name"));
        if !valid_name(&name) { return Err(ToolError::expected("name may only use letters, digits, - and _")); }
        let model = check_model(&ctx, &input["model"]).await?;
        let mut args = json!({"model":model,"name":name});
        if let Some(title) = input.get("title") {
            // String.slice counts UTF-16 code units.
            let units: Vec<_> = string(Some(title)).encode_utf16().take(60).collect();
            args["title"] = json!(String::from_utf16_lossy(&units));
        }
        if let Some(material) = check_material(input.get("material"))? { args["material"] = json!(material); }
        args["projection"] = input.get("projection").filter(|v| !v.is_null()).cloned().unwrap_or(json!("first"));
        args["sheet"] = input.get("sheet").filter(|v| !v.is_null()).cloned().unwrap_or(json!("auto"));
        args["dxf"] = json!(truthy(&input["dxf"]));
        let res = run_freecad("freecad_drawing.py", &ctx, args).await?;
        let mut files = files(&res["files"]);
        let svg = files[0].clone();
        let pdf_note = if let Some(rsvg) = find_program(&["rsvg-convert".into()]) {
            let pdf = svg.strip_suffix(".svg").map(|s| format!("{s}.pdf")).unwrap_or(svg.clone());
            let r = run(&rsvg, &["-f".into(), "pdf".into(), "-o".into(), project_path(&ctx.project_dir, &pdf)?.to_string_lossy().into_owned(), project_path(&ctx.project_dir, &svg)?.to_string_lossy().into_owned()], &ctx.project_dir, 60_000, &[]).await;
            if r.code == 0 { files.insert(1, pdf); String::new() } else { format!("\nPDF export failed: {}", clip(&r.stderr, 500)) }
        } else { "\nNo PDF: install rsvg-convert (Fedora: sudo dnf install librsvg2-tools) to also get a PDF.".into() };
        let holes = res["holes"].as_array().unwrap();
        let table = holes.iter().map(|h| format!("  {}: {}, axis {}, {}{}", string(h.get("tag")), string(h.get("hole")), string(h.get("axis")), string(h.get("at")), if truthy(&h["for"]) { format!(" ({})", string(h.get("for"))) } else { String::new() })).collect::<Vec<_>>().join("\n");
        let mass = if truthy(&res["mass_g"]) { format!(", {} g", string(res.get("mass_g"))) } else { String::new() };
        let table = if holes.is_empty() { String::new() } else { format!("\nHole table:\n{table}") };
        Ok(ToolResult { text: format!("Drawing on {} at {}, {} projection, part {} mm{mass}.{table}{pdf_note}\nFiles: {}", string(res.get("sheet")), string(res.get("scale")), string(res.get("projection")), joined(&res["size_mm"], " × "), files.join(", ")), files })
    })
}
