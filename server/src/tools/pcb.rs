use super::{exec::{report, run}, input_str, paths::project_path, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use serde_json::{json, Value};

mod antenna;
mod route;
mod pour;
mod footprint;
mod place;
mod spec;

pub use footprint::{footprint, Footprint, Pad};
pub use place::{place, PlacedPart};
pub use spec::{parse, prepare, Board, Hole, Keepout, Kind, Layer, Net, NetClass, Part, Pin, Pour, PreparedBoard, Rules, Spec, Trace};

pub fn tools() -> Vec<Tool> {
    vec![
        {
            let mut tool = Tool::new("pcb", "pcb_drc", r#"Run KiCad's design rule check on a board and write the report to fab/drc.rpt."#, json!({"type":"object", "additionalProperties": false,"properties":{"board":{"type":"string","description":"Project-relative .kicad_pcb file"}},"required":["board"]}), pcb_drc);
            tool.requires = vec!["kicad-cli".into()];
            tool.install = Some(serde_json::from_value(json!({"dnf":"kicad","apt":"kicad","brew":"--cask kicad","url":"https://www.kicad.org/download/"})).unwrap());
            tool
        },
        {
            let mut tool = Tool::new("pcb", "pcb_export_fab", r#"Export manufacturing files for a board: Gerbers and drill files into fab/, ready to zip and upload to a PCB fab such as JLCPCB or PCBWay."#, json!({"type":"object", "additionalProperties": false,"properties":{"board":{"type":"string","description":"Project-relative .kicad_pcb file"}},"required":["board"]}), pcb_export_fab);
            tool.requires = vec!["kicad-cli".into()];
            tool.install = Some(serde_json::from_value(json!({"dnf":"kicad","apt":"kicad","brew":"--cask kicad","url":"https://www.kicad.org/download/"})).unwrap());
            tool
        },
    ]
}

fn pcb_drc(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let board = input_str(&input, "board")?;
        project_path(&ctx.project_dir, board)?;
        project_path(&ctx.project_dir, "fab/drc.rpt")?;
        let r = run("kicad-cli", &["pcb".into(), "drc".into(), "--output".into(), "fab/drc.rpt".into(), board.into()], &ctx.project_dir, 120_000, &[]).await;
        Ok(ToolResult { text: report(&r), files: vec!["fab/drc.rpt".into()] })
    })
}

fn pcb_export_fab(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let board = input_str(&input, "board")?;
        project_path(&ctx.project_dir, board)?;
        project_path(&ctx.project_dir, "fab/")?;
        let gerbers = run("kicad-cli", &["pcb".into(), "export".into(), "gerbers".into(), "--output".into(), "fab/".into(), board.into()], &ctx.project_dir, 120_000, &[]).await;
        if gerbers.code != 0 { return Err(ToolError::expected(report(&gerbers))); }
        let drill = run("kicad-cli", &["pcb".into(), "export".into(), "drill".into(), "--output".into(), "fab/".into(), board.into()], &ctx.project_dir, 120_000, &[]).await;
        Ok(ToolResult { text: format!("gerbers:\n{}\n\ndrill:\n{}", report(&gerbers), report(&drill)), files: vec!["fab/".into()] })
    })
}


pub struct NativeBuild {
    pub report: String,
    pub files: Vec<(String, String)>,
}

/// CPU-only native build. The caller writes these files through project_path after this returns.
pub fn build(json: &str, name: &str, source: &str) -> Result<NativeBuild, String> {
    let prepared = prepare(json)?;
    let board = route::Board::from_prepared(&prepared)?;
    let routed = route::route(&board);
    let errors: Vec<_> = board
        .nets
        .iter()
        .enumerate()
        .filter(|(i, _)| !routed.routed[*i])
        .map(|(_, n)| {
            format!(
                "unroutable: net {} ({} to {})",
                n.name,
                board.pads[n.pads[0]].name,
                board.pads[*n.pads.last().unwrap()].name
            )
        })
        .collect();
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let pours: Vec<_> = prepared
        .spec
        .pours
        .iter()
        .map(|p| {
            let net = board.nets.iter().position(|n| n.name == p.net).unwrap();
            (
                net,
                p.layer,
                pour::pour(&board, &routed, net, p.layer.index()),
            )
        })
        .collect();
    let files = vec![
        (
            format!("pcb/{name}.circuit.json"),
            crate::jsfmt::stringify_pretty(&circuit(&prepared, &board, &routed, &pours))
                .map_err(|e| e.to_string())?,
        ),
        (
            format!("pcb/{name}.board.json"),
            crate::jsfmt::stringify_pretty(&boxes(&prepared)).map_err(|e| e.to_string())?,
        ),
        (
            format!("fab/{name}/{name}-B_Cu.gbr"),
            bottom_copper(&board, &routed, &pours),
        ),
    ];
    let n = crate::jsfmt::number;
    let mut lines = vec![
        format!(
            "Built {source}: board {} × {} mm, {} mm thick, {} copper layers.",
            n(board.size[0]),
            n(board.size[1]),
            n(prepared.spec.board.thickness),
            board.layers
        ),
        format!(
            "{} part(s): {}",
            prepared.parts.len(),
            prepared
                .spec
                .parts
                .iter()
                .map(|p| p.reference.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        format!(
            "{} net(s) routed as {} trace(s) with {} via(s); {} hole(s).",
            board.nets.len(),
            routed.tracks.len() + board.fixed.len(),
            routed.vias.len(),
            board.holes.len()
        ),
    ];
    if !pours.is_empty() {
        lines.push(format!(
            "Copper pours: {}",
            prepared
                .spec
                .pours
                .iter()
                .map(|p| format!("{} on {}", p.net, p.layer.name()))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    lines.push("No errors: every connection is routed.".into());
    lines.push(format!(
        "Files: {}",
        files
            .iter()
            .map(|(p, _)| p.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Ok(NativeBuild {
        report: lines.join("\n"),
        files,
    })
}

type Pours = Vec<(usize, Layer, pour::Shapes)>;

fn boxes(b: &PreparedBoard) -> Value {
    let bounds = b.spec.board.bounds();
    let t = b.spec.board.thickness;
    let components: Vec<_> = b.parts.iter().map(|p| {
        let [x0,y0,x1,y1] = p.footprint.body;
        let corners = [[x0,y0],[x1,y0],[x1,y1],[x0,y1]].map(|q|p.point([q[0],-q[1]]));
        let minx = corners.iter().map(|p|p[0]).fold(f64::INFINITY,f64::min);
        let miny = corners.iter().map(|p|p[1]).fold(f64::INFINITY,f64::min);
        let maxx = corners.iter().map(|p|p[0]).fold(f64::NEG_INFINITY,f64::max);
        let maxy = corners.iter().map(|p|p[1]).fold(f64::NEG_INFINITY,f64::max);
        let (z0,z1) = if p.side == Layer::Top {(t/2.,t/2.+p.footprint.height)} else {(-t/2.-p.footprint.height,-t/2.)};
        json!({"name":b.spec.parts[p.part].reference,"layer":p.side.name(),"min":[minx,miny,z0],"max":[maxx,maxy,z1]})
    }).collect();
    json!({"size":[bounds[2]-bounds[0],bounds[3]-bounds[1],t],"center":[(bounds[0]+bounds[2])/2.,(bounds[1]+bounds[3])/2.],"outline":b.spec.board.polygon(),"components":components})
}

fn circuit(prepared: &PreparedBoard, b: &route::Board, r: &route::Routes, pours: &Pours) -> Value {
    let point = |p: route::Point| json!({"x":p[0],"y":p[1]});
    let net = |i: usize| b.nets.get(i).map(|n| n.name.as_str());
    let mut elements = vec![
        json!({"type":"pcb_board","pcb_board_id":"board","center":{"x":b.origin[0]+b.size[0]/2.,"y":b.origin[1]+b.size[1]/2.},"width":b.size[0],"height":b.size[1],"thickness":prepared.spec.board.thickness,"num_layers":b.layers,"outline":b.outline.iter().map(|p|point(*p)).collect::<Vec<_>>()}),
    ];
    for p in &prepared.parts {
        let spec = &prepared.spec.parts[p.part];
        let id = format!("pcb_component_{}", p.part);
        let source = format!("source_component_{}", p.part);
        elements.push(json!({"type":"source_component","source_component_id":source,"name":spec.reference,"value":spec.value}));
        elements.push(json!({"type":"pcb_component","pcb_component_id":id,"source_component_id":source,"center":point(p.at),"rotation":p.rotation,"layer":p.side.name(),"width":p.footprint.body[2]-p.footprint.body[0],"height":p.footprint.body[3]-p.footprint.body[1]}));
        let prefix = format!("{}.", spec.reference);
        for pad in b.pads.iter().filter(|pad| pad.name.starts_with(&prefix)) {
            let pin = pad.name.strip_prefix(&prefix).unwrap();
            if pad.drill > 0. {
                elements.push(json!({"type":"pcb_plated_hole","pcb_plated_hole_id":pad.name,"pcb_component_id":id,"x":pad.at[0],"y":pad.at[1],"shape":"circle","outer_diameter":pad.size[0],"hole_diameter":pad.drill,"layers":(0..b.layers).map(|i|Layer::from_index(i).name()).collect::<Vec<_>>(),"port_hints":[format!("pin{pin}"),pin],"net":net(pad.net)}));
            } else {
                elements.push(json!({"type":"pcb_smtpad","pcb_smtpad_id":pad.name,"pcb_component_id":id,"x":pad.at[0],"y":pad.at[1],"width":pad.size[0],"height":pad.size[1],"rotation":pad.rotation,"layer":p.side.name(),"shape":"rect","port_hints":[format!("pin{pin}"),pin],"net":net(pad.net)}));
            }
        }
    }
    for (i, t) in r
        .tracks
        .iter()
        .chain(b.fixed.iter().map(|f| &f.track))
        .enumerate()
    {
        let wires = [t.a,t.b].map(|p|json!({"route_type":"wire","x":p[0],"y":p[1],"width":t.width,"layer":Layer::from_index(t.layer).name()}));
        let mut e = json!({"type":"pcb_trace","pcb_trace_id":format!("trace_{i}"),"width":t.width,"net":net(t.net),"route":wires});
        if i >= r.tracks.len() {
            e["is_antenna_trace"] = json!(true);
            e["pcb_component_id"] = json!(format!(
                "pcb_component_{}",
                b.fixed[i - r.tracks.len()].part
            ));
        }
        elements.push(e);
    }
    for (i, v) in r.vias.iter().enumerate() {
        elements.push(json!({"type":"pcb_via","pcb_via_id":format!("via_{i}"),"x":v.at[0],"y":v.at[1],"outer_diameter":b.via[0],"hole_diameter":b.via[1],"layers":(0..b.layers).map(|l|Layer::from_index(l).name()).collect::<Vec<_>>(),"net":net(v.net)}));
    }
    for (net, layer, shapes) in pours {
        for shape in shapes {
            let ring = |c: &pour::Contour| json!({"vertices":c.iter().map(|p|point(*p)).collect::<Vec<_>>()});
            elements.push(json!({"type":"pcb_copper_pour","pcb_copper_pour_id":format!("pour_{}",elements.len()),"layer":layer.name(),"net":b.nets[*net].name,"shape":"brep","brep_shape":{"outer_ring":ring(&shape[0]),"inner_rings":shape[1..].iter().map(ring).collect::<Vec<_>>()}}));
        }
    }
    Value::Array(elements)
}

fn gerber_coord(p: route::Point, op: usize) -> String {
    format!(
        "X{}Y{}D{op:02}*\n",
        (p[0] * 1e6).round() as i64,
        (p[1] * 1e6).round() as i64
    )
}
fn gerber_region(out: &mut String, shape: &[pour::Contour]) {
    out.push_str("G36*\n");
    for ring in shape {
        out.push_str(&gerber_coord(ring[0], 2));
        for p in ring.iter().skip(1).chain(std::iter::once(&ring[0])) {
            out.push_str(&gerber_coord(*p, 1));
        }
    }
    out.push_str("G37*\n");
}
fn gerber_attribute(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_+- .".contains(c) {
                c.to_string()
            } else {
                format!("\\u{:04X}", c as u32)
            }
        })
        .collect()
}

/// Bottom copper includes the real pads, routed traces, vias, antenna copper and holed pours.
fn bottom_copper(b: &route::Board, r: &route::Routes, pours: &Pours) -> String {
    use std::collections::BTreeMap;
    let mut out = format!("G04 Forge native PCB*\n%FSLAX46Y46*%\n%MOMM*%\n%TF.GenerationSoftware,Forge,pcb,{}*%\n%TF.FileFunction,Copper,L{},Bot*%\n%TF.FilePolarity,Positive*%\n%LPD*%\nG01*\n",env!("CARGO_PKG_VERSION"),b.layers);
    let mut apertures = BTreeMap::new();
    apertures.insert((b.via[0] * 1e6).round() as i64, 0);
    for t in r
        .tracks
        .iter()
        .chain(b.fixed.iter().map(|f| &f.track))
        .filter(|t| t.layer == 1)
    {
        apertures.insert((t.width * 1e6).round() as i64, 0);
    }
    for p in b.pads.iter().filter(|p| p.round && p.layers & 2 != 0) {
        apertures.insert((p.size[0] * 1e6).round() as i64, 0);
    }
    for (i, (width, id)) in apertures.iter_mut().enumerate() {
        *id = i + 10;
        out.push_str(&format!(
            "%TA.AperFunction,Conductor*%\n%ADD{id}C,{}*%\n%TD*%\n",
            crate::jsfmt::fixed_string(*width as f64 / 1e6, 6)
        ));
    }
    out.push_str("D10*\n");
    let attribute = |net: usize| {
        format!(
            "%TO.N,{}*%\n",
            gerber_attribute(b.nets.get(net).map_or("N/C", |n| n.name.as_str()))
        )
    };
    for (net, layer, shapes) in pours {
        if *layer != Layer::Bottom {
            continue;
        }
        out.push_str(&attribute(*net));
        for shape in shapes {
            gerber_region(&mut out, shape);
        }
        out.push_str("%TD*%\n");
    }
    for p in b.pads.iter().filter(|p| p.layers & 2 != 0) {
        out.push_str(&attribute(p.net));
        out.push_str(&format!(
            "%TO.C,{}*%\n",
            gerber_attribute(p.name.split('.').next().unwrap())
        ));
        if p.round {
            out.push_str(&format!(
                "D{}*\n",
                apertures[&((p.size[0] * 1e6).round() as i64)]
            ));
            out.push_str(&gerber_coord(p.at, 3));
        } else {
            let ring = pour::rect([0.; 2], p.size)
                .into_iter()
                .map(|q| p.point(q))
                .collect();
            gerber_region(&mut out, &[ring]);
        }
        out.push_str("%TD*%\n");
    }
    for t in r
        .tracks
        .iter()
        .chain(b.fixed.iter().map(|f| &f.track))
        .filter(|t| t.layer == 1)
    {
        out.push_str(&attribute(t.net));
        out.push_str(&format!(
            "D{}*\n",
            apertures[&((t.width * 1e6).round() as i64)]
        ));
        out.push_str(&gerber_coord(t.a, 2));
        out.push_str(&gerber_coord(t.b, 1));
        out.push_str("%TD*%\n");
    }
    for v in &r.vias {
        out.push_str(&attribute(v.net));
        out.push_str(&format!(
            "D{}*\n",
            apertures[&((b.via[0] * 1e6).round() as i64)]
        ));
        out.push_str(&gerber_coord(v.at, 3));
        out.push_str("%TD*%\n");
    }
    out.push_str("M02*\n");
    out
}

#[cfg(test)]
mod native_tests {
    use super::*;

    #[test]
    fn native_outputs_repeat_and_keep_antenna_feed_and_holes() {
        let json = r#"{"board":{"width":50,"height":40},"parts":[{"ref":"ANT1","kind":"antenna","shape":"2.4ghz_quarter_wave_monopole","at":[-20,18.4]}],"pours":[{"net":"GND","layer":"bottom"}],"keepouts":[{"rect":[-3,16,32,8],"layers":["bottom"]}]}"#;
        let a = build(json, "ant", "pcb/ant.pcb.json").unwrap();
        let b = build(json, "ant", "pcb/ant.pcb.json").unwrap();
        assert_eq!(a.report, b.report);
        assert_eq!(a.files, b.files);
        let circuit: Value = serde_json::from_str(&a.files[0].1).unwrap();
        let elements = circuit.as_array().unwrap();
        let arm = elements
            .iter()
            .find(|e| e["is_antenna_trace"] == true)
            .unwrap();
        assert_eq!(arm["route"][0]["x"], -20.);
        assert_eq!(arm["route"][1]["x"], 11.);
        let feed = elements.iter().find(|e| e["type"] == "pcb_smtpad").unwrap();
        assert_eq!(feed["port_hints"][0], "pin1");
        assert_eq!(feed["pcb_component_id"], arm["pcb_component_id"]);
        let pours: Vec<_> = elements
            .iter()
            .filter(|e| e["type"] == "pcb_copper_pour")
            .collect();
        assert!(!pours.is_empty());
        for p in pours {
            assert_eq!(p["shape"], "brep");
            assert!(p["brep_shape"]["inner_rings"].is_array());
        }
        let gerber = &a.files[2].1;
        assert!(gerber.contains("%FSLAX46Y46*%"));
        assert!(gerber.contains("%MOMM*%"));
        assert!(gerber.contains("%TF.FileFunction,Copper,L2,Bot*%"));
        assert_eq!(
            gerber.matches("G36*").count(),
            gerber.matches("G37*").count()
        );
        assert!(gerber.ends_with("M02*\n"));
        assert_eq!(
            gerber_attribute("GND,*%\\"),
            "GND\\u002C\\u002A\\u0025\\u005C"
        );
    }
}
