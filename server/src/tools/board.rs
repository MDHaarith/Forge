use super::{exec::{clip, run_isolated}, paths::project_path, script3d::{files, parse_runner_output, valid_name}, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::jsfmt::{string, trim, truthy};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn tools() -> Vec<Tool> {
    vec![
        {
            let mut tool = Tool::new("pcb", "pcb_build", r#"Build a printed circuit board natively from pcb/<name>.pcb.json: validate connections, generate footprints, place parts, autoroute 2/4 layers and pour copper. Write JSON with write_file, then call pcb_build. Units mm and degrees; board-centred origin, +y up. Unknown keys are errors.
Format (optional fields may be omitted):
{"board":{"width":30,"height":20,"layers":2,"thickness":1.6},"rules":{"trace":0.2,"clearance":0.2,"via":[0.6,0.3],"edge":0.3},"netclasses":{"power":{"nets":["VCC","GND"],"trace":0.4}},"parts":[{"ref":"R1","kind":"resistor","value":"1k","footprint":"0402","at":[-6,0]},{"ref":"LED1","kind":"led","value":"red","footprint":"0603","at":[6,0],"rotation":90},{"ref":"J1","kind":"header","footprint":"pinheader_1x2","at":[0,-6]},{"ref":"U1","kind":"chip","footprint":"soic8","pins":{"1":"VCC","8":"GND"},"side":"bottom","lcsc":"C123"}],"nets":{"IN":["J1.1","R1.1"],"A":["R1.2","LED1.A"],"GND":["LED1.K","J1.2"]},"holes":[{"at":[12,7],"d":3.2}],"pours":[{"net":"GND","layer":"bottom"}],"keepouts":[{"rect":[-3,6,24,6],"layers":["bottom"]}],"traces":[{"net":"RF","width":1.1}]}
board.outline:[[x,y],...] replaces the rectangular width/height shape; layers is 2 or 4. Thickness defaults to 1.6. Rules default as above; netclasses override trace width; traces overrides a named net's width. Pours and keepouts accept top, bottom, inner1, inner2 (inner layers need 4 layers). Keepout rect is [cx,cy,w,h].
Parts: ref and kind required. kind: resistor capacitor inductor led diode crystal button switch chip header antenna. Omit at to auto-place; rotation defaults 0; side defaults top, or bottom. value and lcsc populate the BOM. Footprints: 0402 0603 0805 1206, sot23 sot23_5 sot223, soicN (even 8..16), tssopN (even 8..28), qfnN (multiples of 4, 16..48, exposed pad), dipN (even 4..40), bgaN (square, 0.8 pitch), pinheader_1xN and pinheader_2xN (2.54 pitch), crystal_hc49 crystal_3225 button_6x6 sod123.
Nets contain <ref>.<pin> endpoints. Numeric pins always work; led/diode aliases A and K work. pins maps numbers to chip labels; endpoints may use those labels. Unconnected labelled pins join their named nets; explicit nets take priority. Unknown pins and overlapping/off-board parts are errors.
Antennas use kind:antenna and shape instead of footprint: 2.4ghz_quarter_wave_monopole, 2.4ghz_meandered_monopole, 2.4ghz_inverted_f, 2.4ghz_meandered_inverted_f, 2.4ghz_folded_dipole. Pin 1 is the feed; inverted-F pin 2 is the plated ground short, connect to GND; folded dipole feeds across pins 1 and 2 through a balun. Put the antenna on an edge. Keep ground under the feed pad but add keepouts on every poured layer covering the radiating arm to the edge, with at least 5 mm clearance starting past the feed. Free-space arm lengths resonate low on FR-4: simulate with rf_antenna_sim after building. Use a pi matching network for tuning.
Outputs: pcb/<name>-pcb.svg, -schematic.svg (grid symbols with net labels), .glb, .step, .circuit.json, .board.json (part boxes for design_assemble); fab/<name>/ with Gerber X2 copper (including inner layers), mask, paste, silkscreen, outline, PTH/NPTH drills, JLCPCB pick-and-place and BOM. Native builds stay offline; lcsc supplies part numbers. Fix every reported error and rebuild until every connection routes. Legacy .tsx/.jsx sources still build through the existing Node engine."#, json!({"type":"object", "additionalProperties": false,"properties":{"source":{"type":"string","description":"Project-relative .pcb.json board file, e.g. pcb/main.pcb.json (.tsx/.jsx still supported)"},"name":{"type":"string","description":"Base name for the outputs (default: the source file's name)"},"lookup_parts":{"type":"boolean","description":"Look up JLCPCB part numbers for the BOM online (default false: the build stays offline)"}},"required":["source"]}), pcb_build);
            tool.requires = vec!["engines/pcb/dist/pcb-engine.mjs".into()];
            tool.needs = Some("Forge's PCB engine");
            tool.install = Some(serde_json::from_value(json!({"url":"npm run pcb-engine"})).unwrap());
            tool
        },
    ]
}

/// The built-in PCB engine, bundled into one file by `npm run pcb-engine` (the desktop app ships it).
pub fn pcb_engine(app_dir: &Path) -> Option<PathBuf> {
    let bundle = super::paths::lexical_path(&app_dir.join("engines/pcb/dist/pcb-engine.mjs"));
    bundle.exists().then_some(bundle)
}

pub fn node_program(program: Option<&Path>) -> String {
    program.map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "node".into())
}

// Warnings every quick board has and that say nothing about whether it works.
const QUIET: &[&str] = &["source_unnamed_trace_warning", "schematic_missing_sheet_warning"];

/// Groups the engine's warnings by kind, so 30 identical "no part number" lines read as one.
pub fn summarize_warnings(warnings: &[String], looked_up: bool) -> Vec<String> {
    let mut groups: Vec<(&str, Vec<&str>)> = Vec::new();
    for w in warnings {
        let kind = w.split(':').next().unwrap();
        if QUIET.contains(&kind) { continue; }
        let message = trim(w.get(kind.len() + 1..).unwrap_or(""));
        if let Some((_, messages)) = groups.iter_mut().find(|(k, _)| *k == kind) { messages.push(message); }
        else { groups.push((kind, vec![message])); }
    }
    let mut lines = Vec::new();
    for (kind, messages) in groups {
        if kind == "source_part_not_found_warning" && !looked_up { continue; }
        if kind == "source_missing_manufacturer_part_number_warning" {
            lines.push(format!("{} part(s) have no manufacturer part number, so the BOM has no LCSC numbers{}", messages.len(), if looked_up { "" } else { " (part lookup is off; pass lookup_parts: true to look them up on JLCPCB, or set supplierPartNumbers)" }));
            continue;
        }
        lines.push(if messages.len() == 1 { format!("{kind}: {}", messages[0]) } else { format!("{kind} ×{}, e.g. {}", messages.len(), messages[0]) });
    }
    lines
}

/// Runs the engine in a child Node that may only read the engine and the project, write the project, and start nothing.
pub async fn run_engine(engine: &Path, ctx: &ToolContext, args: Value, timeout_ms: u64) -> Result<super::exec::ExecResult, ToolError> {
    let project = project_path(&ctx.project_dir, "")?.to_string_lossy().into_owned();
    let engine = engine.to_string_lossy();
    let node = vec!["--permission".into(), format!("--allow-fs-read={engine}"), format!("--allow-fs-read={project}"), format!("--allow-fs-write={project}"), "--disable-warning=ExperimentalWarning".into(), "--disable-warning=SecurityWarning".into(), engine.into_owned()];
    // Only what the engine needs: the board code must not see Forge's API key.
    let mut env: Vec<_> = ctx.node_env.iter().filter(|(key, _)| matches!(key.as_str(), "PATH" | "LANG")).cloned().collect();
    env.push(("FORGE_ARGS".into(), serde_json::to_string(&args)?));
    Ok(run_isolated(&node_program(ctx.node_program.as_deref()), &node, &ctx.project_dir, timeout_ms, &env, &format!("the board took longer than {} s to build (a large autoroute?)", crate::jsfmt::number(timeout_ms as f64 / 1000.0))).await)
}

fn pcb_build(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let source = string(Some(input.get("source").filter(|v| !v.is_null()).unwrap_or(&json!(""))));
        let full = project_path(&ctx.project_dir, &source)?;
        let native = source.ends_with(".pcb.json");
        if !native && !source.ends_with(".tsx") && !source.ends_with(".jsx") { return Err(ToolError::expected("source must be a .pcb.json file")); }
        if !tokio::fs::metadata(&full).await.is_ok_and(|m| m.is_file()) { return Err(ToolError::expected(format!("{source} does not exist; write it with write_file first"))); }
        let name = input.get("name").map(|v| string(Some(v))).unwrap_or_else(|| {
            let basename = Path::new(&source).file_name().unwrap_or_default().to_string_lossy();
            if native { return basename.strip_suffix(".pcb.json").unwrap().into(); }
            basename.rsplit_once('.').filter(|(_, ext)| !ext.is_empty()).map_or(basename.to_string(), |(base, _)| base.into())
        });
        if !valid_name(&name) { return Err(ToolError::expected("name may only use letters, digits, - and _")); }
        if native {
            let json = tokio::fs::read_to_string(&full).await?;
            let build_name = name.clone();
            let build_source = source.clone();
            let build = tokio::task::spawn_blocking(move || super::pcb::build(&json, &build_name, &build_source)).await
                .map_err(|e| ToolError::unexpected(e.to_string()))?.map_err(ToolError::expected)?;
            let mut files = Vec::new();
            for (file, content) in build.files.into_iter().map(|(p, s)| (p, s.into_bytes())).chain(build.binary_files) {
                let path = project_path(&ctx.project_dir, &file)?;
                tokio::fs::create_dir_all(path.parent().unwrap()).await?;
                tokio::fs::write(path, content).await?;
                files.push(file);
            }
            return Ok(ToolResult { text: build.report, files });
        }
        let engine = pcb_engine(&ctx.app_dir).ok_or_else(|| ToolError::expected("Forge's PCB engine is not installed: run npm run pcb-engine in the Forge folder"))?;
        let looked_up = truthy(&input["lookup_parts"]);
        let root = project_path(&ctx.project_dir, "")?;
        let rel = full.strip_prefix(root).unwrap().to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        let r = run_engine(&engine, &ctx, json!({"source":rel,"name":name,"lookup_parts":looked_up}), 5 * 60_000).await?;
        let parsed = parse_runner_output(&r)?;
        if let Some(error) = parsed.error { return Err(ToolError::expected(clip(&format!("PCB engine: {error}"), 12_000))); }
        let res = parsed.result.unwrap();
        let b = &res["board"];
        let parts = files(&res["components"]);
        let errors = files(&res["errors"]);
        let warnings = summarize_warnings(&files(&res["warnings"]), looked_up);
        let mut lines = vec![
            format!("Built {rel}: board {} × {} mm, {} mm thick, {} copper layers.", string(b.get("width")), string(b.get("height")), string(b.get("thickness")), string(b.get("layers"))),
            format!("{} part(s): {}{}", parts.len(), parts.iter().take(60).cloned().collect::<Vec<_>>().join(", "), if parts.len() > 60 { " …" } else { "" }),
            format!("{} connection(s) and {} named net(s), routed as {} trace(s) with {} via(s); {} hole(s).", string(res.get("connections")), string(res.get("nets")), string(res.get("traces")), string(res.get("vias")), string(res.get("holes"))),
        ];
        let pours = res.get("pours").filter(|v| !v.is_null()).map(files).unwrap_or_default();
        if !pours.is_empty() || truthy(&res["keepouts"]) { lines.push(format!("Copper pours: {}; {} keepout area(s).", if pours.is_empty() { "none".into() } else { pours.join(", ") }, string(Some(res.get("keepouts").filter(|v| !v.is_null()).unwrap_or(&json!(0)))))); }
        if !errors.is_empty() { lines.push(format!("{} ERROR(S), fix them and rebuild:", errors.len())); lines.extend(errors.iter().take(30).map(|e| format!("  {e}"))); }
        else { lines.push("No errors: every connection is routed.".into()); }
        if !warnings.is_empty() { lines.push("Warnings:".into()); lines.extend(warnings.iter().take(20).map(|w| format!("  {w}"))); }
        if truthy(&res["schematicNote"]) { lines.push(string(res.get("schematicNote"))); }
        let mut files = files(&res["files"]).into_iter().filter(|f| f.starts_with("pcb/")).collect::<Vec<_>>();
        lines.push(format!("Files: {}, fab/{name}/ (Gerbers, drills, pick-and-place, BOM)", files.join(", ")));
        files.push(format!("fab/{name}/"));
        Ok(ToolResult { text: clip(&lines.join("\n"), 12_000), files })
    })
}
