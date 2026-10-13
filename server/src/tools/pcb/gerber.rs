use super::{pour, route, Layer, Pours, PreparedBoard};
use crate::jsfmt::fixed_string;

fn header(function: &str) -> String {
    // Mask geometry describes openings, so its file polarity is negative; drawing polarity stays dark.
    let polarity = if function.starts_with("Soldermask,") {
        "Negative"
    } else {
        "Positive"
    };
    format!("G04 Forge native PCB*\n%FSLAX46Y46*%\n%MOMM*%\n%TF.GenerationSoftware,Forge,pcb,{}*%\n%TF.FileFunction,{function}*%\n%TF.FilePolarity,{polarity}*%\n%LPD*%\nG01*\n", env!("CARGO_PKG_VERSION"))
}

fn pad(out: &mut String, p: &route::Pad, expansion: f64) {
    let size = p.size.map(|v| v + 2. * expansion);
    let ring = if p.round {
        pour::circle(p.at, size[0] / 2.)
    } else {
        pour::rect([0.; 2], size)
            .into_iter()
            .map(|q| p.point(q))
            .collect()
    };
    region(out, &[ring]);
}

fn openings(b: &route::Board, r: &route::Routes, layer: Layer, paste: bool) -> String {
    let side = if layer == Layer::Top { "Top" } else { "Bot" };
    let mut out = header(&format!(
        "{},{side}",
        if paste { "Paste" } else { "Soldermask" }
    ));
    out.push_str("%ADD10C,0.1*%\nD10*\n");
    let expansion = if paste { 0. } else { 0.05 };
    for p in b
        .pads
        .iter()
        .filter(|p| p.layers & (1 << layer.index()) != 0 && (!paste || p.drill == 0.))
    {
        pad(&mut out, p, expansion);
    }
    if !paste {
        for v in &r.vias {
            region(&mut out, &[pour::circle(v.at, b.via[0] / 2. + expansion)]);
        }
    }
    out.push_str("M02*\n");
    out
}

fn silk(prepared: &PreparedBoard, layer: Layer) -> String {
    let mut out = header(&format!(
        "Legend,{}",
        if layer == Layer::Top { "Top" } else { "Bot" }
    ));
    out.push_str("%ADD10C,0.15*%\nD10*\n");
    for p in prepared.parts.iter().filter(|p| p.side == layer) {
        let outline: Vec<_> = p
            .footprint
            .silkscreen
            .iter()
            .map(|q| p.point([q[0], -q[1]]))
            .collect();
        if let Some(first) = outline.first() {
            out.push_str(&coord(*first, 2));
            for q in outline.iter().skip(1).chain(std::iter::once(first)) {
                out.push_str(&coord(*q, 1));
            }
        }
        let mark = p.footprint.pin1;
        region(
            &mut out,
            &[pour::circle(p.point([mark[0], -mark[1]]), 0.15)],
        );
    }
    out.push_str("M02*\n");
    out
}

fn edge(b: &route::Board) -> String {
    let mut out = header("Profile,NP");
    out.push_str("%ADD10C,0.05*%\nD10*\n");
    out.push_str(&coord(b.outline[0], 2));
    for q in b
        .outline
        .iter()
        .skip(1)
        .chain(std::iter::once(&b.outline[0]))
    {
        out.push_str(&coord(*q, 1));
    }
    out.push_str("M02*\n");
    out
}

pub(super) fn drills(
    b: &route::Board,
    r: &route::Routes,
    plated: bool,
) -> Vec<(route::Point, f64)> {
    if plated {
        b.pads
            .iter()
            .filter(|p| p.drill > 0.)
            .map(|p| (p.at, p.drill))
            .chain(r.vias.iter().map(|v| (v.at, b.via[1])))
            .collect()
    } else {
        b.holes.clone()
    }
}

fn drill(b: &route::Board, r: &route::Routes, plated: bool) -> String {
    use std::collections::BTreeMap;
    let holes = drills(b, r, plated);
    let mut tools: BTreeMap<_, usize> = holes
        .iter()
        .map(|(_, d)| ((d * 1e6).round() as i64, 0))
        .collect();
    let mut out = format!(
        "M48\n; #@! TF.FileFunction,{}Plated,1,{},{}\nMETRIC\n",
        if plated { "" } else { "Non" },
        b.layers,
        if plated { "PTH" } else { "NPTH" }
    );
    for (i, (d, tool)) in tools.iter_mut().enumerate() {
        *tool = i + 1;
        out.push_str(&format!(
            "T{tool:02}C{}\n",
            fixed_string(*d as f64 / 1e6, 6)
        ));
    }
    out.push_str("%\nG90\nM71\n");
    for (d, tool) in tools {
        out.push_str(&format!("T{tool:02}\n"));
        for (p, _) in holes
            .iter()
            .filter(|(_, size)| (size * 1e6).round() as i64 == d)
        {
            // Excellon coordinates need an explicit decimal point, even for whole millimetres.
            let decimal = |v| {
                let text = fixed_string(v, 6);
                if text.contains('.') {
                    text
                } else {
                    format!("{text}.0")
                }
            };
            out.push_str(&format!("X{}Y{}\n", decimal(p[0]), decimal(p[1])));
        }
    }
    out.push_str("M30\n");
    out
}

pub(super) fn files(
    prepared: &PreparedBoard,
    b: &route::Board,
    r: &route::Routes,
    pours: &Pours,
    name: &str,
) -> Vec<(String, String)> {
    let mut files = vec![(
        format!("fab/{name}/{name}-F_Cu.gbr"),
        copper(b, r, pours, Layer::Top),
    )];
    if b.layers == 4 {
        for (suffix, layer) in [("In1_Cu", Layer::Inner1), ("In2_Cu", Layer::Inner2)] {
            files.push((
                format!("fab/{name}/{name}-{suffix}.gbr"),
                copper(b, r, pours, layer),
            ));
        }
    }
    for (prefix, layer) in [("F", Layer::Top), ("B", Layer::Bottom)] {
        for (suffix, text) in [
            ("Mask", openings(b, r, layer, false)),
            ("Paste", openings(b, r, layer, true)),
            ("Silkscreen", silk(prepared, layer)),
        ] {
            files.push((format!("fab/{name}/{name}-{prefix}_{suffix}.gbr"), text));
        }
    }
    files.push((format!("fab/{name}/{name}-Edge_Cuts.gbr"), edge(b)));
    for (suffix, plated) in [("PTH", true), ("NPTH", false)] {
        files.push((
            format!("fab/{name}/{name}-{suffix}.drl"),
            drill(b, r, plated),
        ));
    }
    files
}

fn coord(p: route::Point, op: usize) -> String {
    format!(
        "X{}Y{}D{op:02}*\n",
        (p[0] * 1e6).round() as i64,
        (p[1] * 1e6).round() as i64
    )
}
fn region(out: &mut String, shape: &[pour::Contour]) {
    out.push_str("G36*\n");
    for ring in shape {
        out.push_str(&coord(ring[0], 2));
        for p in ring.iter().skip(1).chain(std::iter::once(&ring[0])) {
            out.push_str(&coord(*p, 1));
        }
    }
    out.push_str("G37*\n");
}
pub(super) fn attribute(s: &str) -> String {
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

/// Copper includes the real pads, routed traces, vias, antenna copper and holed pours.
pub(super) fn copper(b: &route::Board, r: &route::Routes, pours: &Pours, layer: Layer) -> String {
    use std::collections::BTreeMap;
    let index = layer.index();
    let function = match layer {
        Layer::Top => "Copper,L1,Top".into(),
        Layer::Bottom => format!("Copper,L{},Bot", b.layers),
        Layer::Inner1 => "Copper,L2,Inr".into(),
        Layer::Inner2 => "Copper,L3,Inr".into(),
    };
    let mut out = header(&function);
    let mut apertures = BTreeMap::new();
    apertures.insert((b.via[0] * 1e6).round() as i64, 0);
    for t in r
        .tracks
        .iter()
        .chain(b.fixed.iter().map(|f| &f.track))
        .filter(|t| t.layer == index)
    {
        apertures.insert((t.width * 1e6).round() as i64, 0);
    }
    for p in b
        .pads
        .iter()
        .filter(|p| p.round && p.layers & (1 << index) != 0)
    {
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
    let net_attribute = |net: usize| {
        format!(
            "%TO.N,{}*%\n",
            attribute(b.nets.get(net).map_or("N/C", |n| n.name.as_str()))
        )
    };
    for (net, pour_layer, shapes) in pours {
        if *pour_layer != layer {
            continue;
        }
        out.push_str(&net_attribute(*net));
        for shape in shapes {
            region(&mut out, shape);
        }
        out.push_str("%TD*%\n");
    }
    for p in b.pads.iter().filter(|p| p.layers & (1 << index) != 0) {
        out.push_str(&net_attribute(p.net));
        out.push_str(&format!(
            "%TO.C,{}*%\n",
            attribute(p.name.split('.').next().unwrap())
        ));
        if p.round {
            out.push_str(&format!(
                "D{}*\n",
                apertures[&((p.size[0] * 1e6).round() as i64)]
            ));
            out.push_str(&coord(p.at, 3));
        } else {
            pad(&mut out, p, 0.);
        }
        out.push_str("%TD*%\n");
    }
    for t in r
        .tracks
        .iter()
        .chain(b.fixed.iter().map(|f| &f.track))
        .filter(|t| t.layer == index)
    {
        out.push_str(&net_attribute(t.net));
        out.push_str(&format!(
            "D{}*\n",
            apertures[&((t.width * 1e6).round() as i64)]
        ));
        out.push_str(&coord(t.a, 2));
        out.push_str(&coord(t.b, 1));
        out.push_str("%TD*%\n");
    }
    for v in &r.vias {
        out.push_str(&net_attribute(v.net));
        out.push_str(&format!(
            "D{}*\n",
            apertures[&((b.via[0] * 1e6).round() as i64)]
        ));
        out.push_str(&coord(v.at, 3));
        out.push_str("%TD*%\n");
    }
    out.push_str("M02*\n");
    out
}
