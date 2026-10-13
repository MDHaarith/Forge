use super::{paths::project_path, script3d::valid_name, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use crate::jsfmt::string;
use serde_json::{json, Value};
use std::path::Path;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new("pcb", "pcb_build", r#"Build a printed circuit board natively from pcb/<name>.pcb.json: validate connections, generate footprints, place parts, autoroute 2/4 layers and pour copper. Write JSON with write_file, then call pcb_build. Units mm and degrees; board-centred origin, +y up. Unknown keys are errors.
Format (optional fields may be omitted):
{"board":{"width":30,"height":20,"layers":2,"thickness":1.6},"rules":{"trace":0.2,"clearance":0.2,"via":[0.6,0.3],"edge":0.3},"netclasses":{"power":{"nets":["VCC","GND"],"trace":0.4}},"parts":[{"ref":"R1","kind":"resistor","value":"1k","footprint":"0402","at":[-6,0]},{"ref":"LED1","kind":"led","value":"red","footprint":"0603","at":[6,0],"rotation":90},{"ref":"J1","kind":"header","footprint":"pinheader_1x2","at":[0,-6]},{"ref":"U1","kind":"chip","footprint":"soic8","pins":{"1":"VCC","8":"GND"},"side":"bottom","lcsc":"C123"}],"nets":{"IN":["J1.1","R1.1"],"A":["R1.2","LED1.A"],"GND":["LED1.K","J1.2"]},"holes":[{"at":[12,7],"d":3.2}],"pours":[{"net":"GND","layer":"bottom"}],"keepouts":[{"rect":[-3,6,24,6],"layers":["bottom"]}],"traces":[{"net":"RF","width":1.1}]}
board.outline:[[x,y],...] replaces the rectangular width/height shape; layers is 2 or 4. Thickness defaults to 1.6. Rules default as above; netclasses override trace width; traces overrides a named net's width. Pours and keepouts accept top, bottom, inner1, inner2 (inner layers need 4 layers). Keepout rect is [cx,cy,w,h].
Parts: ref and kind required. kind: resistor capacitor inductor led diode crystal button switch chip header antenna. Omit at to auto-place; rotation defaults 0; side defaults top, or bottom. value and lcsc populate the BOM. Footprints: 0402 0603 0805 1206, sot23 sot23_5 sot223, soicN (even 8..16), tssopN (even 8..28), qfnN (multiples of 4, 16..48, exposed pad), dipN (even 4..40), bgaN (square, 0.8 pitch), pinheader_1xN and pinheader_2xN (2.54 pitch), crystal_hc49 crystal_3225 button_6x6 sod123.
Nets contain <ref>.<pin> endpoints. Numeric pins always work; led/diode aliases A and K work. pins maps numbers to chip labels; endpoints may use those labels. Unconnected labelled pins join their named nets; explicit nets take priority. Unknown pins and overlapping/off-board parts are errors.
Antennas use kind:antenna and shape instead of footprint: 2.4ghz_quarter_wave_monopole, 2.4ghz_meandered_monopole, 2.4ghz_inverted_f, 2.4ghz_meandered_inverted_f, 2.4ghz_folded_dipole. Pin 1 is the feed; inverted-F pin 2 is the plated ground short, connect to GND; folded dipole feeds across pins 1 and 2 through a balun. Put the antenna on an edge. Keep ground under the feed pad but add keepouts on every poured layer covering the radiating arm to the edge, with at least 5 mm clearance starting past the feed. Free-space arm lengths resonate low on FR-4: simulate with rf_antenna_sim after building. Use a pi matching network for tuning.
Outputs: pcb/<name>-pcb.svg, -schematic.svg (grid symbols with net labels), .glb, .step, .circuit.json, .board.json (part boxes for design_assemble); fab/<name>/ with Gerber X2 copper (including inner layers), mask, paste, silkscreen, outline, PTH/NPTH drills, JLCPCB pick-and-place and BOM. Native builds stay offline; lcsc supplies part numbers. Fix every reported error and rebuild until every connection routes."#, json!({"type":"object", "additionalProperties": false,"properties":{"source":{"type":"string","description":"Project-relative .pcb.json board file, e.g. pcb/main.pcb.json"},"name":{"type":"string","description":"Base name for the outputs (default: the source file's name)"}},"required":["source"]}), pcb_build),
    ]
}

fn pcb_build(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let source = string(Some(input.get("source").filter(|v| !v.is_null()).unwrap_or(&json!(""))));
        let full = project_path(&ctx.project_dir, &source)?;
        if !source.ends_with(".pcb.json") { return Err(ToolError::expected("source must be a .pcb.json file")); }
        if !tokio::fs::metadata(&full).await.is_ok_and(|m| m.is_file()) { return Err(ToolError::expected(format!("{source} does not exist; write it with write_file first"))); }
        let name = input.get("name").map(|v| string(Some(v))).unwrap_or_else(|| {
            let basename = Path::new(&source).file_name().unwrap_or_default().to_string_lossy();
            basename.strip_suffix(".pcb.json").unwrap().into()
        });
        if !valid_name(&name) { return Err(ToolError::expected("name may only use letters, digits, - and _")); }
        let json = tokio::fs::read_to_string(&full).await?;
        let build = tokio::task::spawn_blocking(move || super::pcb::build(&json, &name, &source)).await
            .map_err(|e| ToolError::unexpected(e.to_string()))?.map_err(ToolError::expected)?;
        let mut files = Vec::new();
        for (file, content) in build.files.into_iter().map(|(p, s)| (p, s.into_bytes())).chain(build.binary_files) {
            let path = project_path(&ctx.project_dir, &file)?;
            tokio::fs::create_dir_all(path.parent().unwrap()).await?;
            tokio::fs::write(path, content).await?;
            files.push(file);
        }
        Ok(ToolResult { text: build.report, files })
    })
}
