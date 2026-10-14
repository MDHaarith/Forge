use super::{input_str, paths::project_path, Tool, ToolContext, ToolError, ToolFuture, ToolResult};
use serde_json::{json, Value};

mod antenna;
mod beautify;
mod drc;
mod export;
mod fab;
mod footprint;
mod gerber;
mod impedance;
mod ipc356;
pub mod kicad;
mod panel;
mod place;
mod pour;
mod route;
mod spec;
mod svg;
mod tune;

pub use footprint::{footprint, Footprint, Pad};
pub use place::{place, PlacedPart};
pub use spec::{
    parse, prepare, Board, DiffPair, Hole, Keepout, Kind, Layer, LengthGroup, Net, NetClass, Part, Pin, Pour,
    Panel, PreparedBoard, Rules, Spec, Trace,
};

pub fn tools() -> Vec<Tool> {
    vec![Tool::new("pcb", "pcb_drc", "Check a built native board for copper clearance, track width, drills, annular rings, edge clearance, unrouted nets, courtyard overlaps and vias in SMD pads. Also lists electrical rule warnings (single-pin nets, unconnected or shorted two-pin parts, unused parts). Reads pcb/<board>.circuit.json and pcb/<board>.pcb.json; writes fab/<board>/<board>-drc.txt. Fix every fault and rebuild before ordering.", json!({"type":"object", "additionalProperties":false,"properties":{"board":{"type":"string","description":"Name of the board built with pcb_build"}},"required":["board"]}), pcb_drc)]
}

fn pcb_drc(input: Value, ctx: ToolContext) -> ToolFuture {
    Box::pin(async move {
        let name = input_str(&input, "board")?;
        if !super::script3d::valid_name(name) {
            return Err(ToolError::expected(
                "name may only use letters, digits, - and _",
            ));
        }
        let circuit_path = project_path(&ctx.project_dir, &format!("pcb/{name}.circuit.json"))?;
        let spec_path = project_path(&ctx.project_dir, &format!("pcb/{name}.pcb.json"))?;
        let output = format!("fab/{name}/{name}-drc.txt");
        let output_path = project_path(&ctx.project_dir, &output)?;
        let missing =
            || ToolError::expected(format!("board {name} is not built; run pcb_build first"));
        let circuit = tokio::fs::read_to_string(circuit_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                missing()
            } else {
                ToolError::from(e)
            }
        })?;
        let spec = tokio::fs::read_to_string(spec_path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                missing()
            } else {
                ToolError::from(e)
            }
        })?;
        let text = tokio::task::spawn_blocking(move || drc::check(&spec, &circuit))
            .await
            .map_err(|e| ToolError::expected(e.to_string()))?
            .map_err(ToolError::expected)?;
        tokio::fs::create_dir_all(output_path.parent().unwrap()).await?;
        tokio::fs::write(output_path, format!("{text}\n")).await?;
        Ok(ToolResult {
            text,
            files: vec![output],
        })
    })
}

pub struct NativeBuild {
    pub report: String,
    pub files: Vec<(String, String)>,
    pub binary_files: Vec<(String, Vec<u8>)>,
}

/// CPU-only native build. The caller writes these files through project_path after this returns.
pub fn build(json: &str, name: &str, source: &str) -> Result<NativeBuild, String> {
    let prepared = prepare(json)?;
    let board = route::Board::from_prepared(&prepared)?;
    let (routed, beautified) = route::route_with_stats(&board);
    let errors: Vec<_> = board
        .nets
        .iter()
        .enumerate()
        .filter(|(i, _)| !routed.routed[*i])
        .map(|(_, n)| {
            if n.pads.is_empty() { return format!("unroutable: net {} (locked copper)", n.name); }
            format!(
                "unroutable: net {} ({} to {})",
                n.name,
                board.pads[n.pads[0]].name,
                board.pads[*n.pads.last().unwrap()].name
            )
        })
        .collect();
    if !errors.is_empty() {
        return Err(format!(
            "{} ERROR(S), fix them and rebuild:\n{}",
            errors.len(),
            errors.join("\n")
        ));
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
    let impedance = impedance::report(&prepared.spec)?;
    let mut files = vec![
        (
            format!("pcb/{name}.circuit.json"),
            export::json(&circuit(&prepared, &board, &routed, &pours))
                .map_err(|e| e.to_string())?,
        ),
        (
            format!("pcb/{name}.board.json"),
            export::json(&boxes(&prepared)).map_err(|e| e.to_string())?,
        ),
        (
            format!("fab/{name}/{name}-B_Cu.gbr"),
            gerber::copper(&board, &routed, &pours, Layer::Bottom),
        ),
    ];
    files.extend(gerber::files(&prepared, &board, &routed, &pours, name));
    files.push((format!("fab/{name}/{name}.ipc"), ipc356::netlist(&prepared, &board, &routed, name)));
    files.push((format!("fab/{name}/{name}-fab.svg"), fab::drawing(&prepared, &board, &routed, name, impedance.as_deref())));
    if prepared.spec.panel.is_some() {
        files.extend(panel::files(&prepared, &board, &routed, &pours, name));
    }
    files.push((
        format!("pcb/{name}-pcb.svg"),
        svg::board(&prepared, &board, &routed, &pours),
    ));
    files.push((
        format!("pcb/{name}-schematic.svg"),
        svg::schematic(&prepared),
    ));
    files.push((format!("fab/{name}/{name}-pnp.csv"), export::pnp(&prepared)));
    files.push((format!("fab/{name}/{name}-bom.csv"), export::bom(&prepared)));
    let (step, glb) = export::model(&prepared, &board, &routed)?;
    files.push((format!("pcb/{name}.step"), step));
    let binary_files = vec![(format!("pcb/{name}.glb"), glb)];
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
            board
                .nets
                .iter()
                .map(|net| net.pads.len().saturating_sub(1))
                .sum::<usize>(),
            routed.vias.len(),
            board.holes.len()
        ),
    ];
    if let Some(s) = beautified.filter(|s| s.changed) {
        lines.push(format!("Beautified: {} -> {} segments, {} via(s) removed, {} corner(s) chamfered.",
            s.before, s.after, s.vias, s.chamfers));
    }
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
    let blind = routed.vias.iter().filter(|v| v.span != (0, board.layers - 1)).count();
    if blind > 0 { lines[2].push_str(&format!(" ({blind} blind/buried)")); }
    if let Some(impedance) = impedance { lines.push(impedance); }
    if let Some(panel) = &prepared.spec.panel { lines.push(panel::summary(panel, &prepared)); }
    for pair in &board.pairs {
        if routed.fallback.contains(&pair.name) {
            lines.push(format!("Diffpair {}: routed as separate nets (could not couple)", pair.name));
        } else {
            let p = tune::total_length(&board, &routed, pair.p);
            let skew = (p - tune::total_length(&board, &routed, pair.n)).abs();
            let coupled = tune::coupled(&routed.tracks, pair.p, pair.n, pair.width + pair.gap).0;
            lines.push(format!("Diffpair {}: coupled {:.0}% of {:.2} mm, skew {:.2} mm, gap {:.2} mm", pair.name,
                if p > 0. { 100. * coupled / p } else { 0. }, p, skew, pair.gap));
        }
    }
    for (name, nets, tolerance) in &board.length_groups {
        let lengths: Vec<_> = nets.iter().map(|&n| tune::total_length(&board, &routed, n)).collect();
        lines.push(format!("Length group {name}: {:.2}-{:.2} mm, tolerance {tolerance:.2} mm",
            lengths.iter().copied().fold(f64::INFINITY, f64::min), lengths.iter().copied().fold(0., f64::max)));
    }
    lines.push("No errors: every connection is routed.".into());
    lines.push(format!(
        "Files: {}",
        files
            .iter()
            .map(|(p, _)| p.as_str())
            .chain(binary_files.iter().map(|(p, _)| p.as_str()))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Ok(NativeBuild {
        report: lines.join("\n"),
        files,
        binary_files,
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
            if let Some(drill) = pad.drill {
                if !pad.plated {
                    let mut e = json!({"type":"pcb_hole","pcb_hole_id":pad.name,"pcb_component_id":id,"x":pad.at[0],"y":pad.at[1],"hole_diameter":drill[0],"hole_width":drill[0],"hole_height":drill[1]});
                    if prepared.spec.footprints.contains_key(spec.footprint.as_deref().unwrap_or("")) { e["rotation"] = json!(pad.rotation); }
                    elements.push(e);
                } else if pad.round && (pad.size[0] != pad.size[1] || drill[0] != drill[1]) {
                    elements.push(json!({"type":"pcb_plated_hole","pcb_plated_hole_id":pad.name,"pcb_component_id":id,"x":pad.at[0],"y":pad.at[1],"shape":"oval","outer_diameter":pad.size[0].max(pad.size[1]),"outer_width":pad.size[0],"outer_height":pad.size[1],"hole_diameter":drill[0].min(drill[1]),"hole_width":drill[0],"hole_height":drill[1],"rotation":pad.rotation,"layers":(0..b.layers).map(|i|Layer::from_index(i).name()).collect::<Vec<_>>(),"port_hints":[format!("pin{pin}"),pin],"net":net(pad.net)}));
                } else {
                    let mut e = json!({"type":"pcb_plated_hole","pcb_plated_hole_id":pad.name,"pcb_component_id":id,"x":pad.at[0],"y":pad.at[1],"shape":if pad.round { "circle" } else { "rect" },"outer_diameter":pad.size[0],"hole_diameter":drill[0],"layers":(0..b.layers).map(|i|Layer::from_index(i).name()).collect::<Vec<_>>(),"port_hints":[format!("pin{pin}"),pin],"net":net(pad.net)});
                    if prepared.spec.footprints.contains_key(spec.footprint.as_deref().unwrap_or("")) {
                        e["outer_width"] = json!(pad.size[0]); e["outer_height"] = json!(pad.size[1]);
                        e["hole_width"] = json!(drill[0]); e["hole_height"] = json!(drill[1]);
                        e["rotation"] = json!(pad.rotation);
                    }
                    elements.push(e);
                }
            } else {
                let mut e = json!({"type":"pcb_smtpad","pcb_smtpad_id":pad.name,"pcb_component_id":id,"x":pad.at[0],"y":pad.at[1],"width":pad.size[0],"height":pad.size[1]});
                if pad.radius > 0. {
                    e["rect_border_radius"] = json!(pad.radius);
                }
                e.as_object_mut().unwrap().extend(json!({"rotation":pad.rotation,"layer":if pad.layers & 1 != 0 { "top" } else { "bottom" },"shape":if pad.round { "circle" } else { "rect" },"port_hints":[format!("pin{pin}"),pin],"net":net(pad.net)}).as_object().unwrap().clone());
                elements.push(e);
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
        if i >= r.tracks.len() && b.fixed[i - r.tracks.len()].part != usize::MAX {
            e["is_antenna_trace"] = json!(true);
            e["pcb_component_id"] = json!(format!(
                "pcb_component_{}",
                b.fixed[i - r.tracks.len()].part
            ));
        }
        elements.push(e);
    }
    for (i, v) in r.vias.iter().enumerate() {
        elements.push(json!({"type":"pcb_via","pcb_via_id":format!("via_{i}"),"x":v.at[0],"y":v.at[1],"outer_diameter":b.via[0],"hole_diameter":b.via[1],"layers":(0..b.layers).filter(|&l|v.on(l,b.layers)).map(|l|Layer::from_index(l).name()).collect::<Vec<_>>(),"net":net(v.net)}));
    }
    for t in &r.teardrops {
        elements.push(json!({"type":"pcb_trace_teardrop","layer":Layer::from_index(t.layer).name(),"net":net(t.net),"points":t.points.iter().map(|p|point(*p)).collect::<Vec<_>>()}));
    }
    for (net, layer, shapes) in pours {
        for shape in shapes {
            let ring = |c: &pour::Contour| json!({"vertices":c.iter().map(|p|point(*p)).collect::<Vec<_>>()});
            elements.push(json!({"type":"pcb_copper_pour","pcb_copper_pour_id":format!("pour_{}",elements.len()),"layer":layer.name(),"net":b.nets[*net].name,"shape":"brep","brep_shape":{"outer_ring":ring(&shape[0]),"inner_rings":shape[1..].iter().map(ring).collect::<Vec<_>>()}}));
        }
    }
    Value::Array(elements)
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
            gerber::attribute("GND,*%\\"),
            "GND\\u002C\\u002A\\u0025\\u005C"
        );
    }
}
