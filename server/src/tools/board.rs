use super::{
    paths::project_path, script3d::valid_name, Tool, ToolContext, ToolError, ToolFuture, ToolResult,
};
use crate::jsfmt::{string, stringify_pretty};
use serde_json::{json, Value};
use std::path::Path;

pub fn tools() -> Vec<Tool> {
    vec![Tool::new(
        "pcb",
        "pcb_build",
        r#"Build a printed circuit board natively from pcb/<name>.pcb.json: validate connections, generate footprints, place parts, autoroute 2/4 layers and pour copper. Write JSON with write_file, then call pcb_build. Units mm and degrees; board-centred origin, +y up. Unknown keys are errors.
Format (optional fields may be omitted):
{"board":{"width":30,"height":20,"layers":2,"thickness":1.6,"er":4.5,"prepreg":0.21},"rules":{"trace":0.2,"clearance":0.2,"via":[0.6,0.3],"edge":0.3,"blind_vias":false,"teardrops":false,"beautify":true,"paste_reduction":0,"paste_windowpane":false},"netclasses":{"power":{"nets":["VCC","GND"],"trace":0.4}},"diffpairs":{},"length_groups":{},"parts":[{"ref":"R1","kind":"resistor","value":"1k","footprint":"0402","at":[-6,0]},{"ref":"LED1","kind":"led","value":"red","footprint":"0603","at":[6,0],"rotation":90},{"ref":"J1","kind":"header","footprint":"pinheader_1x2","at":[0,-6]},{"ref":"U1","kind":"chip","footprint":"soic8","pins":{"1":"VCC","8":"GND"},"side":"bottom","lcsc":"C123"}],"nets":{"IN":["J1.1","R1.1"],"A":["R1.2","LED1.A"],"GND":["LED1.K","J1.2"]},"holes":[{"at":[12,7],"d":3.2}],"pours":[{"net":"GND","layer":"bottom"}],"keepouts":[{"rect":[-3,6,24,6],"layers":["bottom"]}],"traces":[{"net":"RF","width":1.1}]}
board.outline:[[x,y],...] replaces the rectangular width/height shape; layers is 2 or 4. Thickness defaults to 1.6. Rules default as above; netclasses set exactly one of trace or impedance (ohms); traces overrides a named net's width. Impedance uses board.er (4.5), prepreg (0.21 mm on 4 layers), or thickness minus 0.07 on 2 layers. Optional diffpairs:{"USB":{"p":"DP","n":"DN","gap":0.2,"width":0.2,"skew":0.1}} require two-pin nets; use impedance instead of width for differential ohms. Gap defaults to clearance, width to normal widths, skew to 0.1 mm. Optional length_groups:{"DATA":{"nets":["D0","D1"],"tolerance":0.5}} add meanders; groups cannot include diffpairs. rules.blind_vias needs 4 layers; rules.teardrops adds copper tapers. Both default false. rules.beautify defaults true: pull routed copper into neat octilinear paths, chamfer clear corners, reduce vias and centre pad entries before tuning. Set false for raw router output. Pours and keepouts accept top, bottom, inner1, inner2 (inner layers need 4 layers). Keepout rect is [cx,cy,w,h].
rules.paste_reduction (default 0, nonnegative mm per side) shrinks only stencil paste; rules.paste_windowpane (default false) windows SMD pads at least 2 mm wide with 0.3 mm webs and a 60% area target. Openings below 0.1 mm are omitted.
Optional tracks:[{"net":"SDA","layer":"top","width":0.25,"points":[[10,5],[14,5],[16,7]]}] and vias:[{"net":"SDA","at":[16,7],"from":"top","to":"bottom"}] are locked copper. Width defaults to the net width; via from/to default to through. The router fills only missing connections; beautify/tuning never move locked copper. All layer names above apply.
Optional footprints:{"custom":{"pads":[{"number":"1","at":[0,0],"size":[1,1],"shape":"rect","drill":null,"layers":"top","rotation":0}],"courtyard":[[-1,-1],[1,-1],[1,1],[-1,1]],"body":[[-0.5,-0.5],[0.5,-0.5],[0.5,0.5],[-0.5,0.5]],"height":1}} adds y-up footprints. Use footprint:"custom" on parts, or import with pcb_import_footprint first. Shapes: rect, roundrect (optional roundrect_rratio), circle, oval; drill: null, diameter, or [width,height]; layers: top, bottom, through. Built-in name clashes are errors.
Optional panel:{"rows":2,"cols":3,"spacing":2,"rails":5,"rail_sides":"y","separation":"tabs","tabs":2,"tab_width":3,"mousebites":true,"fiducials":true,"tooling":true}. Rows/cols are 1..20; tabs are 1..64 per edge; rectangular boards only. All fields except rows/cols have these defaults. rail_sides is x, y or both. Vscore uses separation:"vscore", default spacing 0 and full-panel score lines. Rails 0 needs fiducials:false and tooling:false. Tab edges need a tab-width corner margin; rails must fit enabled fiducials/tooling.
Parts: ref and kind required. kind: resistor capacitor inductor led diode crystal button switch chip header antenna. Omit at to auto-place; rotation defaults 0; side defaults top, or bottom. value and lcsc populate the BOM. Footprints: 0402 0603 0805 1206, sot23 sot23_5 sot223, soicN (even 8..16), tssopN (even 8..28), qfnN (multiples of 4, 16..48, exposed pad), dipN (even 4..40), bgaN (square, 0.8 pitch), pinheader_1xN and pinheader_2xN (2.54 pitch), crystal_hc49 crystal_3225 button_6x6 sod123, usb_c_16, usb_micro_b, jst_ph_N (2..16), barrel_jack.
Nets contain <ref>.<pin> endpoints. Numeric pins always work; led/diode aliases A and K work. pins maps numbers to chip labels; endpoints may use those labels. Unconnected labelled pins join their named nets; explicit nets take priority. Unknown pins and overlapping/off-board parts are errors.
Antennas use kind:antenna and shape instead of footprint: 2.4ghz_quarter_wave_monopole, 2.4ghz_meandered_monopole, 2.4ghz_inverted_f, 2.4ghz_meandered_inverted_f, 2.4ghz_folded_dipole. Pin 1 is the feed; inverted-F pin 2 is the plated ground short, connect to GND; folded dipole feeds across pins 1 and 2 through a balun. Put the antenna on an edge. Keep ground under the feed pad but add keepouts on every poured layer covering the radiating arm to the edge, with at least 5 mm clearance starting past the feed. Free-space arm lengths resonate low on FR-4: simulate with rf_antenna_sim after building. Use a pi matching network for tuning.
Outputs: pcb/<name>-pcb.svg, -schematic.svg (grid symbols with net labels), .glb, .step, .circuit.json, .board.json (part boxes for design_assemble); fab/<name>/ with Gerber X2 copper (including inner layers), mask, paste, silkscreen, outline, PTH/NPTH and blind/buried drills, .ipc test netlist, -fab.svg drawing, JLCPCB pick-and-place and BOM. Panels add fab/<name>-panel/ with stepped layers/drills, optional -VScore.gbr and -pnp.csv (designators suffixed _r1c1 etc.; rows bottom to top); BOM remains per board. Native builds stay offline; lcsc supplies part numbers. Fix every reported error and rebuild until every connection routes."#,
        json!({"type":"object", "additionalProperties": false,"properties":{"source":{"type":"string","description":"Project-relative .pcb.json board file, e.g. pcb/main.pcb.json"},"name":{"type":"string","description":"Base name for the outputs (default: the source file's name)"}},"required":["source"]}),
        pcb_build,
    ), Tool::new("pcb", "pcb_import_footprint",
        "Import a project-relative KiCad .kicad_mod into a .pcb.json board's inline footprints. Optional name overrides the file's footprint name. Then set a part's footprint to that name and call pcb_build. Imported coordinates use mm, +y up; no KiCad installation is needed.",
        json!({"type":"object","additionalProperties":false,"properties":{"file":{"type":"string","description":"Project-relative KiCad .kicad_mod file"},"board":{"type":"string","description":"Project-relative .pcb.json board to receive the footprint"},"name":{"type":"string","description":"Optional footprint name; defaults to the KiCad footprint name"}},"required":["file","board"]}), pcb_import_footprint)]
}

async fn board_source(input: &Value, ctx: &ToolContext) -> Result<(String, std::path::PathBuf), ToolError> {
    let source = string(Some(input.get("source").filter(|v| !v.is_null()).unwrap_or(&json!(""))));
    let full = project_path(&ctx.project_dir, &source)?;
    if !source.ends_with(".pcb.json") {
        return Err(ToolError::expected("source must be a .pcb.json file"));
    }
    if !tokio::fs::metadata(&full).await.is_ok_and(|m| m.is_file()) {
        return Err(ToolError::expected(format!(
            "{source} does not exist; write it with write_file first"
        )));
    }
    Ok((source, full))
}

fn source_name(source: &str) -> String {
    Path::new(source).file_name().unwrap_or_default().to_string_lossy()
        .strip_suffix(".pcb.json").unwrap().into()
}

pub(crate) fn pcb_build(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let (source, full) = board_source(&input, &ctx).await?;
        let name = input
            .get("name")
            .map(|v| string(Some(v)))
            .unwrap_or_else(|| source_name(&source));
        if !valid_name(&name) {
            return Err(ToolError::expected(
                "name may only use letters, digits, - and _",
            ));
        }
        let json = tokio::fs::read_to_string(&full).await?;
        let build = tokio::task::spawn_blocking(move || super::pcb::build(&json, &name, &source))
            .await
            .map_err(|e| ToolError::unexpected(e.to_string()))?
            .map_err(ToolError::expected)?;
        let mut files = Vec::new();
        for (file, content) in build
            .files
            .into_iter()
            .map(|(p, s)| (p, s.into_bytes()))
            .chain(build.binary_files)
        {
            let path = project_path(&ctx.project_dir, &file)?;
            tokio::fs::create_dir_all(path.parent().unwrap()).await?;
            tokio::fs::write(path, content).await?;
            files.push(file);
        }
        Ok(ToolResult {
            text: build.report,
            files,
        })
    })
}

fn pcb_import_footprint(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let file = super::input_str(&input, "file")?;
        let path = project_path(&ctx.project_dir, file)?;
        let board = super::input_str(&input, "board")?;
        let (_, target) = board_source(&json!({"source":board}), &ctx).await?;
        let text = tokio::fs::read_to_string(path).await?;
        let (original, footprint) = super::pcb::kicad::parse(&text).map_err(ToolError::expected)?;
        let name = input.get("name").and_then(Value::as_str).unwrap_or(&original);
        if super::pcb::footprint(name).is_ok() {
            return Err(ToolError::expected(format!("footprints.{name}: clashes with a built-in footprint")));
        }
        let mut spec: Value = serde_json::from_str(&tokio::fs::read_to_string(&target).await?)?;
        let object = spec.as_object_mut().ok_or_else(|| ToolError::expected("board spec must be an object"))?;
        let footprints = object.entry("footprints").or_insert_with(|| json!({})).as_object_mut()
            .ok_or_else(|| ToolError::expected("footprints must be an object"))?;
        let pads = footprint["pads"].as_array().unwrap();
        let mut bounds = [f64::INFINITY,f64::INFINITY,f64::NEG_INFINITY,f64::NEG_INFINITY];
        for p in pads {
            for i in 0..2 {
                let at = p["at"][i].as_f64().unwrap();
                let (s,c) = p["rotation"].as_f64().unwrap().to_radians().sin_cos();
                let size = c.abs()*p["size"][i].as_f64().unwrap() + s.abs()*p["size"][1-i].as_f64().unwrap();
                bounds[i] = bounds[i].min(at-size/2.); bounds[i+2] = bounds[i+2].max(at+size/2.);
            }
        }
        let report = format!("Imported {name}: {} pad(s), {} × {} mm. Use footprint:\"{name}\" on a part, then run pcb_build.",
            pads.len(), crate::jsfmt::number(bounds[2]-bounds[0]), crate::jsfmt::number(bounds[3]-bounds[1]));
        footprints.insert(name.into(), footprint);
        tokio::fs::write(target, stringify_pretty(&spec)?).await?;
        Ok(ToolResult { text: report, files: vec![board.into()] })
    })
}

pub(crate) async fn edit(input: Value, ctx: ToolContext) -> Result<Value, crate::http::HttpError> {
    use crate::http::error;
    let (source, path) = board_source(&input, &ctx).await.map_err(|e| error(400, e.to_string()))?;
    if ["move", "lock", "unlock"].iter().filter(|k| input.get(**k).is_some()).count() != 1 {
        return Err(error(400, "choose exactly one of move, lock or unlock"));
    }
    let mut spec: Value = serde_json::from_str(&tokio::fs::read_to_string(&path).await
        .map_err(|e| error(500, e.to_string()))?).map_err(|e| error(422, e.to_string()))?;
    let mut unlocked = vec![];
    let remove = |spec: &mut Value, net: &str| {
        for key in ["tracks", "vias"] {
            if let Some(entries) = spec.get_mut(key).and_then(Value::as_array_mut) { entries.retain(|t| t["net"] != net); }
        }
    };
    if let Some(movement) = input.get("move") {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Move {
            #[serde(rename = "ref")]
            reference: String,
            at: [f64; 2],
            rotation: Option<f64>,
            side: Option<String>,
        }
        let movement: Move = serde_json::from_value(movement.clone()).map_err(|e| error(400, e.to_string()))?;
        if movement.at.iter().any(|v| !v.is_finite()) || movement.rotation.is_some_and(|v| !v.is_finite()) {
            return Err(error(400, "coordinates must be finite"));
        }
        if movement.side.as_deref().is_some_and(|s| !matches!(s, "top" | "bottom")) {
            return Err(error(400, "side must be top or bottom"));
        }
        let part = spec["parts"].as_array().and_then(|parts| parts.iter().position(|p| p["ref"] == movement.reference))
            .ok_or_else(|| error(404, format!("no part {}", movement.reference)))?;
        let nets = super::pcb::parse(&spec.to_string()).and_then(|p| p.netlist(&p.footprints()?));
        let affected: std::collections::BTreeSet<String> = match nets {
            Ok(nets) => nets.into_iter().filter(|n| n.pins.iter().any(|pin| pin.part == part)).map(|n| n.name).collect(),
            // Keep edits possible after a failed build, including specs with unknown fields.
            Err(_) => spec["nets"].as_object().into_iter().flat_map(|nets| nets.iter())
                .filter(|(_, pins)| pins.as_array().is_some_and(|pins| pins.iter().any(|p|
                    p.as_str().is_some_and(|p| p.rsplit_once('.').is_some_and(|(r,_)| r == movement.reference)))))
                .map(|(name,_)| name.clone())
                .chain(spec["parts"][part]["pins"].as_object().into_iter().flat_map(|pins| pins.values())
                    .filter_map(Value::as_str).map(str::to_owned)).collect(),
        };
        for net in affected {
            if ["tracks", "vias"].iter().any(|key| spec[*key].as_array().is_some_and(|entries| entries.iter().any(|t| t["net"] == net))) {
                unlocked.push(net.clone());
                remove(&mut spec, &net);
            }
        }
        spec["parts"][part]["at"] = json!(movement.at.map(|v| crate::jsfmt::round(v * 100.) / 100.));
        if let Some(rotation) = movement.rotation { spec["parts"][part]["rotation"] = json!(rotation.rem_euclid(360.)); }
        if let Some(side) = movement.side { spec["parts"][part]["side"] = json!(side); }
    } else {
        let lock = input.get("lock").is_some();
        let net = input[if lock { "lock" } else { "unlock" }].as_str()
            .filter(|n| !n.is_empty()).ok_or_else(|| error(400, "net is required"))?;
        if lock {
            let name = source_name(&source);
            let circuit_path = project_path(&ctx.project_dir, &format!("pcb/{name}.circuit.json"))
                .map_err(|e| error(400, e.to_string()))?;
            let circuit = tokio::fs::read_to_string(circuit_path).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound { error(422, format!("board {name} is not built; run pcb_build first")) }
                else { error(500, e.to_string()) }
            })?;
            let circuit: Value = serde_json::from_str(&circuit).map_err(|e| error(422, e.to_string()))?;
            let mut tracks = vec![];
            let mut vias = vec![];
            for e in circuit.as_array().ok_or_else(|| error(422, "circuit must be an array"))?.iter().filter(|e| e["net"] == net) {
                if e["type"] == "pcb_trace" {
                    if let Some(route) = e["route"].as_array().filter(|p| p.len() >= 2) {
                        for pair in route.windows(2) {
                            tracks.push(json!({"net":net,"layer":pair[0]["layer"],"width":pair[0].get("width").unwrap_or(&e["width"]),
                                "points":[[pair[0]["x"],pair[0]["y"]],[pair[1]["x"],pair[1]["y"]]]}));
                        }
                    }
                } else if e["type"] == "pcb_via" {
                    let layers = e["layers"].as_array().ok_or_else(|| error(422, "via needs layers"))?;
                    let physical = |name: &Value| match name.as_str() { Some("top") => 0, Some("inner1") => 1, Some("inner2") => 2, _ => 3 };
                    let from = layers.iter().min_by_key(|l| physical(l)).ok_or_else(|| error(422, "via needs layers"))?;
                    let to = layers.iter().max_by_key(|l| physical(l)).unwrap();
                    vias.push(json!({"net":net,"at":[e["x"],e["y"]],"from":from,"to":to}));
                }
            }
            if tracks.is_empty() && vias.is_empty() { return Err(error(422, format!("net {net} has no copper"))); }
            remove(&mut spec, net);
            for (key, entries) in [("tracks", tracks), ("vias", vias)] {
                if entries.is_empty() { continue; }
                let object = spec.as_object_mut().ok_or_else(|| error(422, "board spec must be an object"))?;
                object.entry(key).or_insert_with(|| json!([])).as_array_mut()
                    .ok_or_else(|| error(422, format!("{key} must be an array")))?.extend(entries);
            }
        } else { remove(&mut spec, net); }
    }
    tokio::fs::write(path, stringify_pretty(&spec).map_err(|e| error(500, e.to_string()))?).await
        .map_err(|e| error(500, e.to_string()))?;
    let built = pcb_build(json!({"source":source}), ctx).await.map_err(|e| error(422, e.to_string()))?;
    Ok(json!({"report":built.text,"unlocked":unlocked}))
}
