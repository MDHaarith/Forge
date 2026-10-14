use super::{gerber, route, Kind, Layer, PreparedBoard};
use crate::jsfmt::{fixed, fixed_string, json_number, stringify_pretty};
use cadrum::{DVec3, Edge, Solid, Tessellation};
use serde_json::Value;

pub(super) fn json(value: &Value) -> Result<String, serde_json::Error> {
    fn round(value: &mut Value) {
        match value {
            Value::Number(n) if n.is_f64() => *value = json_number(fixed(n.as_f64().unwrap(), 6)),
            Value::Array(a) => a.iter_mut().for_each(round),
            Value::Object(o) => o.values_mut().for_each(round),
            _ => {}
        }
    }
    let mut value = value.clone();
    round(&mut value);
    stringify_pretty(&value)
}

fn field(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.into()
    }
}

pub(super) fn pnp(b: &PreparedBoard) -> String {
    let mut out = String::from("Designator,Mid X,Mid Y,Layer,Rotation\n");
    for p in &b.parts {
        let part = &b.spec.parts[p.part];
        if part.kind == Kind::Antenna {
            continue;
        }
        // JLCPCB's KiCad CPL convention: +y up, positive CCW from KiCad's zero on both sides, top-view XY (no bottom-X negation).
        // Bottom footprint mirroring is already applied by placement; do not mirror coordinates or negate rotation again.
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            field(&part.reference),
            fixed_string(p.at[0], 6),
            fixed_string(p.at[1], 6),
            if p.side == Layer::Top {
                "Top"
            } else {
                "Bottom"
            },
            fixed_string(p.rotation, 6)
        ));
    }
    out
}

pub(super) fn bom(b: &PreparedBoard) -> String {
    let mut out = String::from("Comment,Designator,Footprint,LCSC Part #\n");
    for part in &b.spec.parts {
        if part.kind == Kind::Antenna {
            continue;
        }
        out.push_str(&format!(
            "{},{},{},{}\n",
            field(part.value.as_deref().unwrap_or("")),
            field(&part.reference),
            field(part.footprint.as_deref().unwrap_or("")),
            field(part.lcsc.as_deref().unwrap_or(""))
        ));
    }
    out
}

/// Called only by the native build's spawn_blocking worker. Solids drop before the shared kernel guard.
pub(super) fn model(
    b: &PreparedBoard,
    board: &route::Board,
    r: &route::Routes,
) -> Result<(String, Vec<u8>), String> {
    let _kernel = super::super::part::kernel();
    let build = || -> Result<_, cadrum::Error> {
        let t = b.spec.board.thickness;
        let polygon: Vec<_> = board
            .outline
            .iter()
            .map(|p| DVec3::new(p[0], p[1], -t / 2.))
            .collect();
        let mut slab = Solid::extrude(&Edge::polygon(&polygon)?, DVec3::Z * t)?;
        for hole in gerber::drills(board, r, true)
            .into_iter()
            .chain(gerber::drills(board, r, false))
        {
            let drill = if let Some([p1, p2]) = hole.slot {
                let a = DVec3::new(p1[0], p1[1], -t / 2.);
                let b = DVec3::new(p2[0], p2[1], -t / 2.);
                let axis = (b - a).normalize() * (hole.tool / 2.);
                let normal = DVec3::new(-axis.y, axis.x, 0.);
                let profile = [Edge::line(a - normal, b - normal)?,
                    Edge::arc_3pts(b - normal, b + axis, b + normal)?,
                    Edge::line(b + normal, a + normal)?,
                    Edge::arc_3pts(a + normal, a - axis, a - normal)?];
                Solid::extrude(&profile, DVec3::Z * t)?
            } else {
                Solid::cylinder(hole.tool / 2., DVec3::Z * t).translate(DVec3::new(
                    hole.at[0],
                    hole.at[1],
                    -t / 2.,
                ))
            };
            slab = (&slab - &drill).build()?;
        }
        for via in r.vias.iter().filter(|v| v.span != (0, board.layers - 1)) {
            let levels = [t / 2., t / 2. - b.spec.board.prepreg - 0.035,
                -t / 2. + b.spec.board.prepreg + 0.035, -t / 2.];
            let bottom = levels[via.span.1];
            let height = levels[via.span.0] - bottom;
            let drill = Solid::cylinder(board.via[1] / 2., DVec3::Z * height)
                .translate(DVec3::new(via.at[0], via.at[1], bottom));
            slab = (&slab - &drill).build()?;
        }
        let mut solids = vec![slab.color("#246c47")];
        if !b.spec.tracks.is_empty() || !b.spec.vias.is_empty() {
            // Keep legacy board meshes byte-identical; edited copper uses the same manufacturing geometry.
            let level = |depth| {
                if depth == 0 { t / 2. } else if depth == board.layers - 1 { -t / 2. }
                else if depth == 1 { t / 2. - b.spec.board.prepreg - 0.035 }
                else { -t / 2. + b.spec.board.prepreg + 0.035 }
            };
            for fixed in board.fixed.iter().filter(|f| f.part == usize::MAX) {
                let track = &fixed.track;
                let z = level(route::depth(track.layer, board.layers));
                let height = if track.layer == Layer::Bottom.index() { -0.035 } else { 0.035 };
                let a = DVec3::new(track.a[0],track.a[1],z);
                let end = DVec3::new(track.b[0],track.b[1],z);
                let copper = if (end-a).length() < 1e-9 {
                    Solid::cylinder(track.width / 2., DVec3::Z * height).translate(a)
                } else {
                    let axis = (end-a).normalize() * (track.width / 2.);
                    let normal = DVec3::new(-axis.y,axis.x,0.);
                    let profile = [Edge::line(a-normal,end-normal)?,
                        Edge::arc_3pts(end-normal,end+axis,end+normal)?,
                        Edge::line(end+normal,a+normal)?,
                        Edge::arc_3pts(a+normal,a-axis,a-normal)?];
                    Solid::extrude(&profile, DVec3::Z * height)?
                };
                solids.push(copper.color("#b87333"));
            }
            for via in &board.fixed_vias {
                let bottom = level(via.span.1) - 0.035;
                let height = level(via.span.0) + 0.035 - bottom;
                let outer = Solid::cylinder(board.via[0] / 2., DVec3::Z * height);
                let hole = Solid::cylinder(board.via[1] / 2., DVec3::Z * height);
                solids.push((&outer - &hole).build()?.translate(DVec3::new(via.at[0],via.at[1],bottom)).color("#b87333"));
            }
        }
        for p in &b.parts {
            if b.spec.parts[p.part].kind == Kind::Antenna {
                continue;
            }
            let [x0, y0, x1, y1] = p.footprint.body;
            let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|q| {
                let at = p.point([q[0], -q[1]]);
                DVec3::new(
                    at[0],
                    at[1],
                    if p.side == Layer::Top {
                        t / 2.
                    } else {
                        -t / 2. - p.footprint.height
                    },
                )
            });
            let profile = Edge::polygon(&corners)?;
            solids.push(
                Solid::extrude(&profile, DVec3::Z * p.footprint.height)?.color(
                    if b.spec.parts[p.part].kind == Kind::Led {
                        "#cc3333"
                    } else {
                        "#303038"
                    },
                ),
            );
        }
        let mut step = Vec::new();
        Solid::write_step(&solids, &mut step)?;
        let mut mesh = Solid::mesh(
            &solids,
            Tessellation {
                deflection_linear: 0.05,
                deflection_angular: 0.1,
                relative_linear: false,
            },
        )?;
        // Forge's viewer rotates glTF +90 degrees around X and uses mm: encode [x,z,-y], including normals and edges.
        for p in mesh
            .vertices
            .iter_mut()
            .chain(&mut mesh.normals)
            .chain(&mut mesh.edges)
        {
            *p = DVec3::new(p.x, p.z, -p.y);
        }
        let mut glb = Vec::new();
        mesh.write_gltf_binary(&mut glb)?;
        Ok((step, glb))
    };
    let (step, glb) = build().map_err(|e| e.to_string())?;
    let mut step = String::from_utf8(step).map_err(|e| e.to_string())?;
    // STEP's creation timestamp is metadata, not geometry; keep repeated builds byte-identical.
    if let Some(start) = step.find("FILE_NAME(") {
        if let Some(end) = step[start..].find(";") {
            step.replace_range(
                start..start + end + 1,
                "FILE_NAME('','1970-01-01T00:00:00',(''),(''),'Forge','Forge','');",
            );
        }
    }
    Ok((step, glb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_c_step_slots_remove_exact_stadium_volume() {
        let prepared = super::super::prepare(r#"{"board":{"width":30,"height":20},"parts":[{"ref":"J1","kind":"header","footprint":"usb_c_16","at":[0,0],"rotation":37}]}"#).unwrap();
        let b = route::Board::from_prepared(&prepared).unwrap();
        let r = route::route(&b);
        let holes: Vec<_> = gerber::drills(&b, &r, true).into_iter().chain(gerber::drills(&b, &r, false)).collect();
        assert_eq!(holes.iter().filter(|h| h.slot.is_some()).count(), 4);
        let removed: f64 = holes.iter().map(|h| {
            let radius = h.tool / 2.;
            (std::f64::consts::PI * radius * radius + h.slot.map_or(0., |[a, z]| route::distance(a, z) * h.tool))
                * prepared.spec.board.thickness
        }).sum();
        let (step, _) = model(&prepared, &b, &r).unwrap();
        let _kernel = super::super::super::part::kernel();
        let solids = Solid::read_step(&mut step.as_bytes()).unwrap();
        let actual_removed = 30. * 20. * 1.6 - solids[0].volume();
        assert!((actual_removed - removed).abs() / removed < 0.02, "{actual_removed} != {removed}");
        assert!((actual_removed - removed).abs() < 1e-6);
    }

    #[test]
    fn blind_and_buried_bores_stop_at_physical_depths() {
        let b = super::super::prepare(r#"{"board":{"width":20,"height":10,"layers":4},"rules":{"blind_vias":true},"nets":{"N":[]}}"#).unwrap();
        let board = route::Board::from_prepared(&b).unwrap();
        let r = route::Routes { tracks: vec![], vias: vec![route::Via { at: [-2.,0.], net: 0, span: (0,1) },
            route::Via { at: [2.,0.], net: 0, span: (1,2) }], routed: vec![true], passes: 0, teardrops: vec![], fallback: vec![] };
        let (step, _) = model(&b, &board, &r).unwrap();
        let _kernel = super::super::super::part::kernel();
        let solids = Solid::read_step(&mut step.as_bytes()).unwrap();
        let expected = 20. * 10. * 1.6 - std::f64::consts::PI * 0.15_f64.powi(2) * (0.245 + 1.11);
        assert!((solids[0].volume() - expected).abs() < 1e-6);
    }

    #[test]
    fn outputs_preserve_geometry_frame_drill_split_csv_and_repeated_bytes() {
        let input = r#"{"board":{"outline":[[-12,-8],[15,-8],[15,10],[-12,10]],"layers":4},"parts":[{"ref":"J1","kind":"header","footprint":"pinheader_1x2","at":[-5,2],"rotation":90,"side":"bottom"},{"ref":"R1","kind":"resistor","value":"<x>,\"1k\"","footprint":"0603","at":[5,-3],"rotation":45,"lcsc":"C123"}],"holes":[{"at":[10,5],"d":3.2}]}"#;
        let b = super::super::prepare(input).unwrap();
        let board = route::Board::from_prepared(&b).unwrap();
        let r = route::Routes { teardrops: vec![], fallback: vec![],
            tracks: vec![],
            vias: vec![],
            routed: vec![],
            passes: 1,
        };
        let a = model(&b, &board, &r).unwrap();
        assert_eq!(a, model(&b, &board, &r).unwrap());
        let _kernel = super::super::super::part::kernel();
        let solids = Solid::read_step(&mut a.0.as_bytes()).unwrap();
        assert_eq!(solids.len(), 3);
        let package_volume: f64 = b
            .parts
            .iter()
            .map(|p| {
                (p.footprint.body[2] - p.footprint.body[0])
                    * (p.footprint.body[3] - p.footprint.body[1])
                    * p.footprint.height
            })
            .sum();
        let holes: f64 = gerber::drills(&board, &r, true)
            .into_iter()
            .chain(gerber::drills(&board, &r, false))
            .map(|h| {
                if let Some([p1, p2]) = h.slot {
                    let len = (p1[0] - p2[0]).hypot(p1[1] - p2[1]);
                    (std::f64::consts::PI * (h.tool / 2.).powi(2) + h.tool * len)
                        * b.spec.board.thickness
                } else {
                    std::f64::consts::PI * (h.tool / 2.).powi(2) * b.spec.board.thickness
                }
            })
            .sum();
        assert!(
            (solids.iter().map(|s| s.volume()).sum::<f64>()
                - (27. * 18. * 1.6 - holes + package_volume))
                .abs()
                < 1e-6
        );
        assert_eq!(&a.1[..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(a.1[8..12].try_into().unwrap()) as usize,
            a.1.len()
        );
        let len = u32::from_le_bytes(a.1[12..16].try_into().unwrap()) as usize;
        let glb: Value = serde_json::from_slice(&a.1[20..20 + len]).unwrap();
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for mesh in glb["meshes"].as_array().unwrap() {
            for primitive in mesh["primitives"].as_array().unwrap() {
                let position = primitive["attributes"]["POSITION"].as_u64().unwrap() as usize;
                for i in 0..3 {
                    min[i] = min[i].min(glb["accessors"][position]["min"][i].as_f64().unwrap());
                    max[i] = max[i].max(glb["accessors"][position]["max"][i].as_f64().unwrap());
                }
            }
        }
        assert_eq!([min[0], max[0], min[2], max[2]], [-12., 15., -10., 8.]);
        assert!((min[1] + 0.8 + b.parts[0].footprint.height).abs() < 1e-5);
        assert!((max[1] - 0.8 - b.parts[1].footprint.height).abs() < 1e-5);
        assert!(pnp(&b).contains("J1,-5,2,Bottom,90\n"));
        assert!(bom(&b).contains("\"<x>,\"\"1k\"\"\",R1,0603,C123\n"));
        let svg = super::super::svg::schematic(&b);
        assert!(svg.contains("&lt;x&gt;,&quot;1k&quot;"));
        let json = json(&serde_json::json!({"x":-0.9750000000000001,"y":-0.0})).unwrap();
        assert_eq!(json, "{\n  \"x\": -0.975,\n  \"y\": 0\n}");
        let fab = gerber::files(&b, &board, &r, &vec![], "frame");
        let file = |suffix: &str| {
            &fab.iter()
                .find(|(name, _)| name.ends_with(suffix))
                .unwrap()
                .1
        };
        assert!(file("PTH.drl").contains("T01C1\n"));
        assert!(!file("PTH.drl").contains("C3.2"));
        assert!(file("NPTH.drl").contains("T01C3.2\n%\nG90\nM71\nT01\nX10.0Y5.0\n"));
        assert!(file("In1_Cu.gbr").contains("%TF.FileFunction,Copper,L2,Inr*%"));
        assert!(file("In2_Cu.gbr").contains("%TF.FileFunction,Copper,L3,Inr*%"));
        assert_eq!(file("B_Paste.gbr").matches("G36*").count(), 0);
        assert_eq!(file("F_Paste.gbr").matches("G36*").count(), 0);
        assert_eq!(file("F_Paste.gbr").matches("D03*").count(), 2);
        assert_eq!(file("F_Paste.gbr").matches("%AMRoundRect*").count(), 1);
        assert!(file("F_Mask.gbr").contains("%TF.FilePolarity,Negative*%"));
        assert!(file("B_Mask.gbr").contains("%TF.FilePolarity,Negative*%"));
    }
}
