use super::{pour, route, Layer, Pours, PreparedBoard, Rules};
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
    // Keep the manufacturing format revision stable across app version bumps.
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

fn paste_pads(p: &route::Pad, rules: &Rules) -> Vec<route::Pad> {
    let size = p.size.map(|v| v - 2. * rules.paste_reduction);
    // shortcut: omit stencil openings below 0.1 mm, revisit for a finer stencil process.
    if size.iter().any(|v| *v < 0.1 - 1e-9) { return vec![]; }
    let mut opening = p.clone();
    opening.size = size;
    opening.radius = (p.radius - rules.paste_reduction).max(0.).min(size[0].min(size[1]) / 2.);
    if !rules.paste_windowpane || p.size[0].min(p.size[1]) < 2. { return vec![opening]; }
    let count = size.map(|v| (v / 1.5).ceil().max(1.) as usize);
    let available = [(size[0] - (count[0] - 1) as f64 * 0.3) / count[0] as f64,
        (size[1] - (count[1] - 1) as f64 * 0.3) / count[1] as f64];
    let target = (0.6 * p.size[0] * p.size[1] / (count[0] * count[1]) as f64).sqrt();
    // shortcut: large reductions cap coverage below 60%, add process-limit validation when a minimum paste area is required.
    let scale = (target / (available[0] * available[1]).sqrt()).min(1.);
    let window = available.map(|v| v * scale);
    if window.iter().any(|v| *v < 0.1 - 1e-9) { return vec![]; }
    let mut windows = vec![];
    for y in 0..count[1] {
        for x in 0..count[0] {
            let mut pad = opening.clone();
            pad.at = p.point([(x as f64 - (count[0] - 1) as f64 / 2.) * (window[0] + 0.3),
                (y as f64 - (count[1] - 1) as f64 / 2.) * (window[1] + 0.3)]);
            pad.size = window;
            pad.radius = 0.;
            pad.round = false;
            windows.push(pad);
        }
    }
    windows
}

fn openings(b: &route::Board, r: &route::Routes, layer: Layer, paste: bool, rules: &Rules) -> String {
    use std::collections::BTreeMap;
    let side = if layer == Layer::Top { "Top" } else { "Bot" };
    let mut out = header(&format!(
        "{},{side}",
        if paste { "Paste" } else { "Soldermask" }
    ));
    let expansion = if paste { 0. } else { 0.05 };
    let pads: Vec<_> = b.pads.iter()
        .filter(|p| p.layers & (1 << layer.index()) != 0 && (!paste || p.drill.is_none()))
        .flat_map(|p| if paste && (rules.paste_reduction > 0. || rules.paste_windowpane) {
            paste_pads(p, rules)
        } else { vec![p.clone()] }).collect();
    let mut apertures = BTreeMap::new();
    if !paste {
        apertures.insert(
            Aperture::Circle(((b.via[0] + 2. * expansion) * 1e6).round() as i64),
            0,
        );
    }
    for p in pads.iter().filter_map(|p| flash(p, expansion))
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
    for p in &pads
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
        for v in r.vias.iter().filter(|v| v.on(layer.index(), b.layers)) {
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
    contours("Profile,NP", std::slice::from_ref(&b.outline), true)
}

pub(super) fn contours(function: &str, contours: &[pour::Contour], closed: bool) -> String {
    let mut out = header(function);
    out.push_str("%ADD10C,0.05*%\nD10*\n");
    for ring in contours {
        out.push_str(&coord(ring[0], 2));
        for q in ring.iter().skip(1) { out.push_str(&coord(*q, 1)); }
        if closed { out.push_str(&coord(ring[0], 1)); }
    }
    out.push_str("M02*\n");
    out
}

pub(super) fn fiducials(out: &mut String, points: &[route::Point], diameter: f64) {
    let id = out.lines().filter_map(|line| line.strip_prefix("%ADD"))
        .filter_map(|line| line.split(|c: char| !c.is_ascii_digit()).next()?.parse::<usize>().ok())
        .max().unwrap_or(10) + 1;
    out.truncate(out.len() - "M02*\n".len());
    out.push_str(&format!("%ADD{id}C,{}*%\nD{id}*\n", fixed_string(diameter, 6)));
    for at in points { out.push_str(&coord(*at, 3)); }
    out.push_str("M02*\n");
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
        pads.chain(r.vias.iter().filter(|v| v.span == (0, b.layers - 1)).map(|v| DrillHole {
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
    let holes = drills(b, r, plated);
    drill_file(&holes, &format!("{}Plated,1,{},{}", if plated { "" } else { "Non" }, b.layers, if plated { "PTH" } else { "NPTH" }))
}

pub(super) fn drill_file(holes: &[DrillHole], function: &str) -> String {
    use std::collections::BTreeMap;
    let mut tools: BTreeMap<_, usize> = holes
        .iter()
        .map(|h| ((h.tool * 1e6).round() as i64, 0))
        .collect();
    let mut out = format!("M48\n; #@! TF.FileFunction,{function}\nMETRIC\n");
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
            ("Mask", openings(b, r, layer, false, &prepared.spec.rules)),
            ("Paste", openings(b, r, layer, true, &prepared.spec.rules)),
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
    let mut spans = std::collections::BTreeMap::<_, Vec<DrillHole>>::new();
    for v in r.vias.iter().filter(|v| v.span != (0, b.layers - 1)) {
        spans.entry(v.span).or_default().push(DrillHole { at: v.at, tool: b.via[1], slot: None });
    }
    for ((a, z), holes) in spans {
        let (a, z) = (a + 1, z + 1);
        let kind = if a == 1 || z == b.layers { "Blind" } else { "Buried" };
        files.push((format!("fab/{name}/{name}-PTH-{a}-{z}.drl"), drill_file(&holes, &format!("Plated,{a},{z},{kind}"))));
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
    for (i, ring) in shape.iter().enumerate() {
        // Gerber fills each contour; holes need separate clear-polarity regions.
        if i == 1 { out.push_str("%LPC*%\n"); }
        out.push_str("G36*\n");
        out.push_str(&coord(ring[0], 2));
        for p in ring.iter().skip(1).chain(std::iter::once(&ring[0])) {
            out.push_str(&coord(*p, 1));
        }
        out.push_str("G37*\n");
    }
    if shape.len() > 1 { out.push_str("%LPD*%\n"); }
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
    for v in r.vias.iter().filter(|v| v.on(index, b.layers)) {
        out.push_str(&net_attribute(v.net));
        out.push_str(&format!(
            "D{}*\n",
            apertures[&Aperture::Circle((b.via[0] * 1e6).round() as i64)]
        ));
        out.push_str(&coord(v.at, 3));
        out.push_str("%TD*%\n");
    }
    for t in r.teardrops.iter().filter(|t| t.layer == index) {
        out.push_str(&net_attribute(t.net));
        region(&mut out, std::slice::from_ref(&t.points));
        out.push_str("%TD*%\n");
    }
    out.push_str("M02*\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windowpane_has_sixty_percent_coverage_and_rotates_with_pad() {
        let prepared = super::super::prepare(r#"{"board":{"width":20,"height":20},"rules":{"paste_windowpane":true},"parts":[{"ref":"U1","kind":"chip","footprint":"qfn32","at":[0,0],"rotation":37}]}"#).unwrap();
        let b = route::Board::from_prepared(&prepared).unwrap();
        let r = route::route(&b);
        let pad = b.pads.iter().find(|p| p.size[0].min(p.size[1]) >= 2.).unwrap();
        let windows = paste_pads(pad, &prepared.spec.rules);
        let count = pad.size.map(|s| (s / 1.5).ceil() as usize);
        assert_eq!(windows.len(), count[0] * count[1]);
        let coverage = windows.iter().map(|p| p.size[0] * p.size[1]).sum::<f64>() / (pad.size[0] * pad.size[1]);
        assert!((0.5..=0.7).contains(&coverage));
        assert!((coverage - 0.6).abs() < 1e-9);
        assert_eq!(openings(&b, &r, Layer::Top, true, &prepared.spec.rules).matches("G36*").count(), windows.len());
        for window in &windows {
            assert_eq!(window.rotation, pad.rotation);
            for corner in pour::rect([0.; 2], window.size) {
                let local = pad.local(window.point(corner));
                assert!(local.iter().zip(pad.size).all(|(v, size)| v.abs() < size / 2. + 1e-9));
            }
        }
        let local = pad.local(windows[0].at);
        assert!((local[0] - (windows[0].at[0] - pad.at[0])).abs() > 0.1);
        let mut two_mm = pad.clone(); two_mm.size = [2.; 2];
        let windows = paste_pads(&two_mm, &prepared.spec.rules);
        assert!((windows.iter().map(|p| p.size[0] * p.size[1]).sum::<f64>() / 4. - 0.6).abs() < 1e-9);
    }

    #[test]
    fn paste_reduction_shrinks_radius_and_size_without_changing_mask() {
        let prepared = super::super::prepare(r#"{"board":{"width":20,"height":20},"parts":[{"ref":"R1","kind":"resistor","footprint":"0603","at":[0,0]}]}"#).unwrap();
        let b = route::Board::from_prepared(&prepared).unwrap();
        let r = route::route(&b);
        let mut rules = prepared.spec.rules.clone(); rules.paste_reduction = 0.05;
        let p = &b.pads[0];
        let opening = paste_pads(p, &rules).remove(0);
        for axis in 0..2 { assert!((opening.size[axis] - (p.size[axis] - 0.1)).abs() < 1e-9); }
        assert!((opening.radius - (p.radius - 0.05)).abs() < 1e-9);
        let mut out = String::new();
        let Some(Aperture::RoundRect(args)) = flash(&opening, 0.) else { panic!("missing rounded stencil aperture"); };
        roundrect_aperture(&mut out, 11, &args);
        assert!(openings(&b, &r, Layer::Top, true, &rules).contains(&out));
        assert_eq!(openings(&b, &r, Layer::Top, false, &rules), openings(&b, &r, Layer::Top, false, &prepared.spec.rules));
        rules.paste_reduction = 10.;
        assert_eq!(openings(&b, &r, Layer::Top, true, &rules).matches("D03*").count(), 0);
    }

    #[test]
    fn buried_vias_emit_only_spanned_copper_and_no_outer_mask() {
        let prepared = super::super::prepare(r#"{"board":{"width":20,"height":10,"layers":4},"rules":{"blind_vias":true},"nets":{"N":[]}}"#).unwrap();
        let b = route::Board::from_prepared(&prepared).unwrap();
        let r = route::Routes { tracks: vec![], vias: vec![route::Via { at: [0.;2], net: 0, span: (1,2) }], routed: vec![true],
            passes: 0, teardrops: vec![], fallback: vec![] };
        for layer in [Layer::Top, Layer::Bottom] {
            assert_eq!(copper(&b, &r, &vec![], layer).matches("D03*").count(), 0);
            assert_eq!(openings(&b, &r, layer, false, &prepared.spec.rules).matches("D03*").count(), 0);
        }
        for layer in [Layer::Inner1, Layer::Inner2] {
            assert_eq!(copper(&b, &r, &vec![], layer).matches("D03*").count(), 1);
        }
        assert!(drills(&b, &r, true).is_empty());
        let files = files(&prepared, &b, &r, &vec![], "buried");
        let drill = files.iter().find(|(n, _)| n.ends_with("PTH-2-3.drl")).unwrap();
        assert!(drill.1.contains("; #@! TF.FileFunction,Plated,2,3,Buried\n"));
    }

    #[test]
    fn holed_regions_clear_copper_and_restore_dark_polarity() {
        let contains = |text: &str, at: route::Point| {
            let (mut dark, mut copper) = (true, false);
            let mut ring = vec![];
            for line in text.lines() {
                match line {
                    "%LPC*%" => dark = false,
                    "%LPD*%" => dark = true,
                    "G36*" => ring.clear(),
                    "G37*" => if super::super::place::inside(at, &ring) { copper = dark; },
                    _ if line.starts_with('X') => {
                        let (x, tail) = line[1..].split_once('Y').unwrap();
                        let (y, op) = tail.split_once('D').unwrap();
                        // Multiple contours in one region are a union, regardless of winding.
                        if op == "02*" && !ring.is_empty() {
                            if super::super::place::inside(at, &ring) { copper = dark; }
                            ring.clear();
                        }
                        ring.push([x.parse::<f64>().unwrap() / 1e6, y.parse::<f64>().unwrap() / 1e6]);
                    }
                    _ => {}
                }
            }
            copper
        };
        let mut out = String::new();
        region(&mut out, &[pour::rect([0.; 2], [20.; 2]), pour::rect([0.; 2], [4.; 2]), pour::circle([5., 0.], 0.5)]);
        assert!(contains(&out, [8., 0.]));
        assert!(!contains(&out, [0.; 2]));
        assert!(!contains(&out, [5., 0.]));
        region(&mut out, &[pour::rect([0.; 2], [1.; 2])]);
        assert!(contains(&out, [0.; 2]));
        assert!(!contains(&out, [1., 0.]));
    }

    #[test]
    fn rotated_roundrects_flash_and_expand_on_mask_and_paste() {
        let prepared = super::super::prepare(r#"{"board":{"width":20,"height":20},"parts":[{"ref":"R1","kind":"resistor","footprint":"0603","rotation":37}]}"#).unwrap();
        let b = route::Board::from_prepared(&prepared).unwrap();
        let r = route::route(&b);
        for (text, expansion) in [(copper(&b, &r, &vec![], Layer::Top), 0.),
            (openings(&b, &r, Layer::Top, false, &prepared.spec.rules), 0.05), (openings(&b, &r, Layer::Top, true, &prepared.spec.rules), 0.)]
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
