use super::{exec::{clip, run_isolated}, paths::project_path, script3d::{files, parse_runner_output, valid_name}, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::jsfmt::{string, trim, truthy};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn tools() -> Vec<Tool> {
    vec![
        {
            let mut tool = Tool::new("pcb", "pcb_build", r#"Build a printed circuit board from tscircuit code with Forge's built-in PCB engine (no KiCad needed): places parts, autoroutes the traces, and writes the PCB view, schematic, 3D model and fab files. The board then shows in the Design view next to the 3D assembly.
Write the board first with write_file as pcb/<name>.tsx: a default-exported React component returning one <board width="50mm" height="30mm"> (sizes in mm; the board is centred on 0,0). Inside it:
  <resistor name="R1" resistance="10k" footprint="0402" pcbX={-5} pcbY={2} /> · <capacitor name="C1" capacitance="100nF" footprint="0603" /> · <led name="LED1" color="red" footprint="0805" /> · <diode> · <inductor> · <crystal> · <pushbutton> · <switch>
  <chip name="U1" footprint="soic8" pinLabels={{ pin1: "VCC", pin8: "GND" }} /> for ICs (footprints: soic8, sot23, sot23_5, qfn20, tssop16, dip8, bga64 ...) and <pinheader name="J1" pinCount={4} pitch="2.54mm" />, <hole diameter="3.2mm" pcbX={..} pcbY={..} /> for mounting holes
  <trace from=".R1 > .pin2" to=".LED1 > .anode" /> connects ports; <trace from=".U1 > .VCC" to="net.VCC" /> joins a named net; <net name="GND" /> declares one
  pcbX/pcbY place a part in mm, pcbRotation turns it, layer="bottom" puts it underneath. A <board> also takes outline={[{x,y},...]} for a custom shape, layers={4} and thickness="1.6mm".
  Ground and RF: <copperpour connectsTo="net.GND" layer="bottom" /> pours a ground plane (one per layer); <keepout pcbX={..} pcbY={..} width="10mm" height="6mm" shape="rect" /> keeps copper out; <trace ... thickness="0.35mm" /> sets a trace width (use rf_calc for a 50 ohm width); <differentialpair positiveConnection=".U1 > .DP" negativeConnection=".U1 > .DN" targetDifferentialImpedance="90ohm" /> pairs USB/Ethernet lines.
  Antennas: <antenna name="ANT1" antennaShape="2.4ghz_inverted_f" /> generates a printed 2.4 GHz antenna (shapes: 2.4ghz_quarter_wave_monopole, 2.4ghz_meandered_monopole, 2.4ghz_inverted_f, 2.4ghz_meandered_inverted_f, 2.4ghz_folded_dipole); .pin1 is the feed. The inverted-F shapes also have .pin2, the ground short: a plated hole into the bottom ground pour (connect it to net.GND); monopoles have only .pin1 and work against the ground pour; the folded dipole is fed between .pin1 and .pin2 (balanced, through a balun). Keep ground copper under the feed pad: it is the antenna's reference. Its own keepout clears only about 1 mm, which detunes it: place it on a board edge and add <keepout ... layers={["bottom"]} /> (and the same for every other layer with a pour) covering the radiating arm out to the board edge with at least 5 mm clearance, starting just past the feed pad. Its arm lengths are free-space quarter waves, which resonate low on FR-4: simulate with rf_antenna_sim after building. Feed it through a pi matching network (three 0402 footprints) so it can be tuned after measurement. For a first product prefer a pre-certified radio module (e.g. ESP32-C3-MINI) whose antenna and certification are already done.
Other .tsx files in the same folder can be imported for reusable blocks. The tool reports the board size, parts, traces, vias and every error (unroutable traces, overlapping parts, bad connections): fix errors and rebuild before calling the board done.
Outputs: pcb/<name>-pcb.svg, pcb/<name>-schematic.svg, pcb/<name>.glb (3D), pcb/<name>.circuit.json, pcb/<name>.board.json (part boxes for design_assemble) and fab/<name>/ with Gerbers, PTH/NPTH drill files, pick-and-place and a BOM in the format JLCPCB accepts."#, json!({"type":"object", "additionalProperties": false,"properties":{"source":{"type":"string","description":"Project-relative .tsx board file, e.g. pcb/main.tsx"},"name":{"type":"string","description":"Base name for the outputs (default: the source file's name)"},"lookup_parts":{"type":"boolean","description":"Look up JLCPCB part numbers for the BOM online (default false: the build stays offline)"}},"required":["source"]}), pcb_build);
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
            let board = tokio::task::spawn_blocking(move || super::pcb::prepare(&json)).await
                .map_err(|e| ToolError::unexpected(e.to_string()))?.map_err(ToolError::expected)?;
            return Ok(ToolResult {
                text: format!("Validated {source}: {} part(s), {} named net(s). Native PCB outputs are not built yet.", board.parts.len(), board.nets.len()),
                files: Vec::new(),
            });
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
