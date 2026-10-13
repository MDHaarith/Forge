use super::{pour, route, Layer, Pours, PreparedBoard};
use crate::jsfmt::fixed_string;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Aperture {
    Circle(i64),
    Obround(i64, i64),
    RoundRect([i64; 9]),
}

#[derive(Clone, Debug)]
pub(super) struct DrillHole {
    pub at: route::Point,
    pub tool: f64,
    pub slot: Option<[route::Point; 2]>,
}

/// Flash aperture for a circle, oval or rounded rectangle; otherwise draw a region.
fn flash(p: &route::Pad, expansion: f64) -> Option<Aperture> {
    if p.radius > 0. {
        let [w, h] = p.size.map(|v| v + 2. * expansion);
        let r = (p.radius + expansion).max(0.).min(w.min(h) / 2.);
        let (s, c) = p.rotation.to_radians().sin_cos();
        let mut args = [0; 9];
        args[0] = (r * 1e6).round() as i64;
        for (i, [x, y]) in [[-w/2.+r, -h/2.+r], [w/2.-r, -h/2.+r],
            [w/2.-r, h/2.-r], [-w/2.+r, h/2.-r]].into_iter().enumerate()
        {
            args[1 + 2*i] = ((c*x - s*y) * 1e6).round() as i64;
            args[2 + 2*i] = ((s*x + c*y) * 1e6).round() as i64;
        }
        return Some(Aperture::RoundRect(args));
    }
    let [w, h] = p
        .effective_size()
        .map(|v| ((v + 2. * expansion) * 1e6).round() as i64);
    // Obround apertures cannot rotate, so ovals off the 90 degree grid stay regions.
    let square = (p.rotation / 90.).fract().abs() < 1e-9;
    match (p.round, w == h) {
        (true, true) => Some(Aperture::Circle(w)),
        (true, false) if square => Some(Aperture::Obround(w, h)),
        _ => None,
    }
}

fn roundrect_macro(out: &mut String, apertures: &std::collections::BTreeMap<Aperture, usize>) {
    if apertures.keys().any(|a| matches!(a, Aperture::RoundRect(_))) {
        // KiCad's corner-centre macro: fill the centre, corners and strips between corners.
        out.push_str("%AMRoundRect*\n4,1,4,$2,$3,$4,$5,$6,$7,$8,$9,$2,$3,0*\n1,1,$1+$1,$2,$3*\n1,1,$1+$1,$4,$5*\n1,1,$1+$1,$6,$7*\n1,1,$1+$1,$8,$9*\n20,1,$1+$1,$2,$3,$4,$5,0*\n20,1,$1+$1,$4,$5,$6,$7,0*\n20,1,$1+$1,$6,$7,$8,$9,0*\n20,1,$1+$1,$8,$9,$2,$3,0*\n%\n");
    }
}

fn roundrect_aperture(out: &mut String, id: usize, args: &[i64; 9]) {
    out.push_str(&format!("%ADD{id}RoundRect,{}*%\n", args.iter()
        .map(|v| fixed_string(*v as f64 / 1e6, 6)).collect::<Vec<_>>().join("X")));
}

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
        let [w, h] = size;
        if (w - h).abs() < 1e-9 {
            pour::circle(p.at, w / 2.)
        } else if h > w {
            let half = (h - w) / 2.;
            pour::capsule(p.point([0., -half]), p.point([0., half]), w / 2.)
        } else {
            let half = (w - h) / 2.;
            pour::capsule(p.point([-half, 0.]), p.point([half, 0.]), h / 2.)
        }
    } else {
        pour::rect([0.; 2], size)
            .into_iter()
            .map(|q| p.point(q))
            .collect()
    };
    region(out, &[ring]);
}

fn openings(b: &route::Board, r: &route::Routes, layer: Layer, paste: bool) -> String {
    use std::collections::BTreeMap;
    let side = if layer == Layer::Top { "Top" } else { "Bot" };
    let mut out = header(&format!(
        "{},{side}",
        if paste { "Paste" } else { "Soldermask" }
    ));
    let expansion = if paste { 0. } else { 0.05 };
    let mut apertures = BTreeMap::new();
    if !paste {
        apertures.insert(
            Aperture::Circle(((b.via[0] + 2. * expansion) * 1e6).round() as i64),
            0,
        );
    }
    for p in b
        .pads
        .iter()
        .filter(|p| p.layers & (1 << layer.index()) != 0 && (!paste || p.drill.is_none()))
        .filter_map(|p| flash(p, expansion))
    {
        apertures.insert(p, 0);
    }
    out.push_str("%ADD10C,0.1*%\n");
    roundrect_macro(&mut out, &apertures);
    for (i, (aper, id)) in apertures.iter_mut().enumerate() {
        *id = i + 11;
        match aper {
            Aperture::Circle(width) => {
                out.push_str(&format!(
                    "%ADD{id}C,{}*%\n",
                    crate::jsfmt::fixed_string(*width as f64 / 1e6, 6)
                ));
            }
            Aperture::Obround(w, h) => {
                out.push_str(&format!(
                    "%ADD{id}O,{}X{}*%\n",
                    crate::jsfmt::fixed_string(*w as f64 / 1e6, 6),
                    crate::jsfmt::fixed_string(*h as f64 / 1e6, 6)
                ));
            }
            Aperture::RoundRect(args) => roundrect_aperture(&mut out, *id, args),
        }
    }
    out.push_str("D10*\n");
    for p in b
        .pads
        .iter()
        .filter(|p| p.layers & (1 << layer.index()) != 0 && (!paste || p.drill.is_none()))
    {
        if let Some(aper) = flash(p, expansion) {
            out.push_str(&format!("D{}*\n", apertures[&aper]));
            out.push_str(&coord(p.at, 3));
        } else {
            pad(&mut out, p, expansion);
        }
    }
    if !paste {
        let via_aper = Aperture::Circle(((b.via[0] + 2. * expansion) * 1e6).round() as i64);
        out.push_str(&format!("D{}*\n", apertures[&via_aper]));
        for v in &r.vias {
            out.push_str(&coord(v.at, 3));
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

/// A round hole, or a slot drilled with its narrow width along its long axis.
fn pad_hole(p: &route::Pad) -> Option<DrillHole> {
    let [w, h] = p.drill?;
    let half = (w - h).abs() / 2.;
    let slot = (half > 1e-9).then(|| {
        let axis = if h > w { [0., half] } else { [half, 0.] };
        [p.point(axis.map(|v| -v)), p.point(axis)]
    });
    Some(DrillHole {
        at: p.at,
        tool: w.min(h),
        slot,
    })
}

pub(super) fn drills(b: &route::Board, r: &route::Routes, plated: bool) -> Vec<DrillHole> {
    let pads = b
        .pads
        .iter()
        .filter(|p| p.plated == plated)
        .filter_map(pad_hole);
    if plated {
        pads.chain(r.vias.iter().map(|v| DrillHole {
            at: v.at,
            tool: b.via[1],
            slot: None,
        }))
        .collect()
    } else {
        b.holes
            .iter()
            .map(|&(at, tool)| DrillHole {
                at,
                tool,
                slot: None,
            })
            .chain(pads)
            .collect()
    }
}

fn drill(b: &route::Board, r: &route::Routes, plated: bool) -> String {
    use std::collections::BTreeMap;
    let holes = drills(b, r, plated);
    let mut tools: BTreeMap<_, usize> = holes
        .iter()
        .map(|h| ((h.tool * 1e6).round() as i64, 0))
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
    let decimal = |v| {
        let text = fixed_string(v, 6);
        if text.contains('.') {
            text
        } else {
            format!("{text}.0")
        }
    };
    for (d, tool) in tools {
        out.push_str(&format!("T{tool:02}\n"));
        for h in holes.iter().filter(|h| (h.tool * 1e6).round() as i64 == d) {
            if let Some([p1, p2]) = h.slot {
                out.push_str(&format!(
                    "X{}Y{}G85X{}Y{}\n",
                    decimal(p1[0]),
                    decimal(p1[1]),
                    decimal(p2[0]),
                    decimal(p2[1])
                ));
            } else {
                out.push_str(&format!("X{}Y{}\n", decimal(h.at[0]), decimal(h.at[1])));
            }
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
    apertures.insert(Aperture::Circle((b.via[0] * 1e6).round() as i64), 0);
    for t in r
        .tracks
        .iter()
        .chain(b.fixed.iter().map(|f| &f.track))
        .filter(|t| t.layer == index)
    {
        apertures.insert(Aperture::Circle((t.width * 1e6).round() as i64), 0);
    }
    for p in b
        .pads
        .iter()
        .filter(|p| p.layers & (1 << index) != 0)
        .filter_map(|p| flash(p, 0.))
    {
        apertures.insert(p, 0);
    }
    roundrect_macro(&mut out, &apertures);
    for (i, (aper, id)) in apertures.iter_mut().enumerate() {
        *id = i + 10;
        match aper {
            Aperture::Circle(width) => {
                out.push_str(&format!(
                    "%TA.AperFunction,Conductor*%\n%ADD{id}C,{}*%\n%TD*%\n",
                    crate::jsfmt::fixed_string(*width as f64 / 1e6, 6)
                ));
            }
            Aperture::Obround(w, h) => {
                out.push_str(&format!(
                    "%TA.AperFunction,Conductor*%\n%ADD{id}O,{}X{}*%\n%TD*%\n",
                    crate::jsfmt::fixed_string(*w as f64 / 1e6, 6),
                    crate::jsfmt::fixed_string(*h as f64 / 1e6, 6)
                ));
            }
            Aperture::RoundRect(args) => {
                out.push_str("%TA.AperFunction,Conductor*%\n");
                roundrect_aperture(&mut out, *id, args);
                out.push_str("%TD*%\n");
            }
        }
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
        if let Some(aper) = flash(p, 0.) {
            out.push_str(&format!("D{}*\n", apertures[&aper]));
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
            apertures[&Aperture::Circle((t.width * 1e6).round() as i64)]
        ));
        out.push_str(&coord(t.a, 2));
        out.push_str(&coord(t.b, 1));
        out.push_str("%TD*%\n");
    }
    for v in &r.vias {
        out.push_str(&net_attribute(v.net));
        out.push_str(&format!(
            "D{}*\n",
            apertures[&Aperture::Circle((b.via[0] * 1e6).round() as i64)]
        ));
        out.push_str(&coord(v.at, 3));
        out.push_str("%TD*%\n");
    }
    out.push_str("M02*\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_roundrects_flash_and_expand_on_mask_and_paste() {
        let prepared = super::super::prepare(r#"{"board":{"width":20,"height":20},"parts":[{"ref":"R1","kind":"resistor","footprint":"0603","rotation":37}]}"#).unwrap();
        let b = route::Board::from_prepared(&prepared).unwrap();
        let r = route::route(&b);
        for (text, expansion) in [(copper(&b, &r, &vec![], Layer::Top), 0.),
            (openings(&b, &r, Layer::Top, false), 0.05), (openings(&b, &r, Layer::Top, true), 0.)]
        {
            assert_eq!(text.matches("%AMRoundRect*").count(), 1);
            assert_eq!(text.matches("D03*").count(), 2);
            assert!(!text.contains("G36*"));
            let Some(Aperture::RoundRect(args)) = flash(&b.pads[0], expansion) else { panic!("not a rounded flash"); };
            assert_eq!(args[0], ((b.pads[0].radius + expansion)*1e6).round() as i64);
            assert!(text.contains(&format!("RoundRect,{}X", fixed_string(args[0] as f64/1e6, 6))));
        }
        let Some(Aperture::RoundRect(args)) = flash(&b.pads[0], -b.pads[0].radius-0.01) else { panic!("not a rounded flash"); };
        assert_eq!(args[0], 0);
    }
}
