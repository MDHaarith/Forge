use crate::{
    pour::{self, Shapes},
    route::{distance, Board, Point, Routes},
};
use std::{collections::BTreeMap, error::Error, fs, path::Path, time::Instant};

#[derive(Clone, Copy, Debug)]
struct Aperture {
    shape: char,
    size: Point,
}
#[derive(Debug)]
enum Primitive {
    Flash(Point, Aperture),
    Line(Point, Point, f64),
    Region(Vec<Vec<Point>>),
}
fn coord(p: Point, op: usize) -> String {
    format!(
        "X{}Y{}D{op:02}*\n",
        (p[0] * 1e6).round() as i64,
        (p[1] * 1e6).round() as i64
    )
}
fn header(function: &str) -> String {
    format!("G04 Forge PCB spike*\n%FSLAX46Y46*%\n%MOMM*%\n%TF.GenerationSoftware,Forge,pcb-spike,0.1*%\n%TF.FileFunction,{function}*%\n%TF.FilePolarity,Positive*%\n%LPD*%\nG01*\n")
}
fn key(shape: char, size: Point) -> (char, i64, i64) {
    (
        shape,
        (size[0] * 1e6).round() as i64,
        (size[1] * 1e6).round() as i64,
    )
}

fn copper(b: &Board, r: &Routes, layer: usize, pour: Option<&Shapes>) -> String {
    let mut out = header(if layer == 0 {
        "Copper,L1,Top"
    } else {
        "Copper,L2,Bot"
    });
    let mut defs = BTreeMap::new();
    for p in &b.pads {
        if p.layers & (1 << layer) != 0 {
            defs.insert(key(if p.round { 'C' } else { 'R' }, p.size), 0);
        }
    }
    for t in &r.tracks {
        if t.layer == layer {
            defs.insert(key('C', [t.width; 2]), 0);
        }
    }
    if !r.vias.is_empty() {
        defs.insert(key('C', [b.via[0]; 2]), 0);
    }
    for (i, (&(shape, x, y), id)) in defs.iter_mut().enumerate() {
        *id = i + 10;
        out.push_str(if shape == 'C' {
            "%TA.AperFunction,Conductor*%\n"
        } else {
            "%TA.AperFunction,SMDPad,CuDef*%\n"
        });
        out.push_str(&format!(
            "%ADD{id}{shape},{:.6}{}*%\n",
            x as f64 / 1e6,
            if shape == 'R' {
                format!("X{:.6}", y as f64 / 1e6)
            } else {
                String::new()
            }
        ));
        out.push_str("%TD*%\n");
    }
    let net = |n: usize| {
        if n == usize::MAX {
            "N/C"
        } else {
            &b.nets[n].name
        }
    };
    for p in &b.pads {
        if p.layers & (1 << layer) == 0 {
            continue;
        }
        let id = defs[&key(if p.round { 'C' } else { 'R' }, p.size)];
        out.push_str(&format!(
            "%TO.N,{}*%\n%TO.C,{}*%\nD{id}*\n",
            net(p.net),
            p.name.split('.').next().unwrap()
        ));
        out.push_str(&coord(p.at, 3));
        out.push_str("%TD*%\n");
    }
    for t in &r.tracks {
        if t.layer != layer {
            continue;
        }
        out.push_str(&format!(
            "%TO.N,{}*%\nD{}*\n",
            net(t.net),
            defs[&key('C', [t.width; 2])]
        ));
        out.push_str(&coord(t.a, 2));
        out.push_str(&coord(t.b, 1));
        out.push_str("%TD*%\n");
    }
    for v in &r.vias {
        out.push_str(&format!(
            "%TO.N,{}*%\nD{}*\n",
            net(v.net),
            defs[&key('C', [b.via[0]; 2])]
        ));
        out.push_str(&coord(v.at, 3));
        out.push_str("%TD*%\n");
    }
    if let Some(shapes) = pour {
        out.push_str("%TO.N,GND*%\n");
        for shape in shapes {
            out.push_str("G36*\n");
            for ring in shape {
                out.push_str(&coord(ring[0], 2));
                for p in ring.iter().skip(1).chain(std::iter::once(&ring[0])) {
                    out.push_str(&coord(*p, 1));
                }
            }
            out.push_str("G37*\n");
        }
        out.push_str("%TD*%\n");
    }
    out.push_str("M02*\n");
    out
}
fn edges(b: &Board) -> String {
    let mut out = header("Profile,NP");
    out.push_str("%TA.AperFunction,Profile*%\n%ADD10C,0.050000*%\n%TD*%\nD10*\n");
    let outline = pour::rect([0.0, 0.0], b.size);
    out.push_str(&coord(outline[0], 2));
    for p in outline.iter().skip(1).chain(std::iter::once(&outline[0])) {
        out.push_str(&coord(*p, 1));
    }
    out.push_str("M02*\n");
    out
}
fn drills(b: &Board, r: &Routes) -> Vec<(Point, f64)> {
    b.pads
        .iter()
        .filter(|p| p.drill > 0.0)
        .map(|p| (p.at, p.drill))
        .chain(r.vias.iter().map(|v| (v.at, b.via[1])))
        .chain(b.holes.iter().copied())
        .collect()
}
fn drill_file(b: &Board, r: &Routes) -> String {
    let holes = drills(b, r);
    let mut tools = BTreeMap::new();
    for &(_, d) in &holes {
        tools.insert((d * 1e6).round() as i64, 0);
    }
    let mut out=String::from("M48\n; Forge spike: combined plated and non-plated drill preview\n; Coordinates have explicit decimals in mm\nMETRIC\n");
    for (i, (&d, id)) in tools.iter_mut().enumerate() {
        *id = i + 1;
        out.push_str(&format!("T{id:02}C{:.6}\n", d as f64 / 1e6));
    }
    out.push_str("%\nG90\nM71\n");
    for (&d, &id) in &tools {
        out.push_str(&format!("T{id:02}\n"));
        for &(p, dd) in &holes {
            if (dd * 1e6).round() as i64 == d {
                out.push_str(&format!("X{:.6}Y{:.6}\n", p[0], p[1]));
            }
        }
    }
    out.push_str("M30\n");
    out
}

fn xy(s: &str, scale: f64) -> Result<Point, Box<dyn Error>> {
    let s = s.strip_prefix('X').ok_or("missing X")?;
    let (x, y) = s.split_once('Y').ok_or("missing Y")?;
    Ok([x.parse::<f64>()? / scale, y.parse::<f64>()? / scale])
}

// shortcut: reader accepts the linear RS-274X subset this writer emits, use a complete parser for imported Gerbers.
fn read_gerber(text: &str) -> Result<Vec<Primitive>, Box<dyn Error>> {
    if !text.contains("%FSLAX46Y46*%") || !text.contains("%MOMM*%") {
        return Err("reader requires mm and 4.6 absolute coordinates".into());
    }
    let mut defs = BTreeMap::new();
    let mut selected = 0;
    let mut at = [0.0, 0.0];
    let mut region: Option<Vec<Vec<Point>>> = None;
    let mut result = vec![];
    for raw in text.lines() {
        let s = raw.trim();
        if s.starts_with("%ADD") {
            let s = s
                .strip_prefix("%ADD")
                .unwrap()
                .strip_suffix("*%")
                .ok_or("bad aperture terminator")?;
            let (left, right) = s.split_once(',').ok_or("bad aperture definition")?;
            let shape = left.chars().last().ok_or("empty aperture")?;
            if !['C', 'R'].contains(&shape) {
                return Err("unsupported aperture".into());
            }
            let id = left[..left.len() - 1].parse::<usize>()?;
            let size = if let Some((x, y)) = right.split_once('X') {
                [x.parse()?, y.parse()?]
            } else {
                let d = right.parse()?;
                [d, d]
            };
            defs.insert(id, Aperture { shape, size });
        } else if s.starts_with('D') {
            selected = s
                .strip_prefix('D')
                .unwrap()
                .strip_suffix('*')
                .ok_or("bad aperture selection")?
                .parse()?;
            if !defs.contains_key(&selected) {
                return Err("undefined aperture".into());
            }
        } else if s == "G36*" {
            if region.is_some() {
                return Err("nested region".into());
            }
            region = Some(vec![]);
        } else if s == "G37*" {
            let shape = region.take().ok_or("region terminator without start")?;
            if shape
                .iter()
                .any(|r| r.len() < 4 || distance(r[0], *r.last().unwrap()) > 1e-8)
            {
                return Err("region must close".into());
            }
            result.push(Primitive::Region(shape));
        } else if s.starts_with('X') {
            let (pos, op) = s
                .strip_suffix('*')
                .ok_or("bad coordinate terminator")?
                .split_once('D')
                .ok_or("missing operation")?;
            let p = xy(pos, 1e6)?;
            match (op, region.as_mut()) {
                ("02", Some(r)) => r.push(vec![p]),
                ("01", Some(r)) => r.last_mut().ok_or("region draw before move")?.push(p),
                ("02", None) => {}
                ("01", None) => {
                    let a = *defs.get(&selected).ok_or("draw without aperture")?;
                    if a.shape != 'C' {
                        return Err("unsupported rectangular draw".into());
                    }
                    result.push(Primitive::Line(at, p, a.size[0]));
                }
                ("03", None) => result.push(Primitive::Flash(
                    p,
                    *defs.get(&selected).ok_or("flash without aperture")?,
                )),
                _ => return Err("unsupported Gerber operation".into()),
            }
            at = p;
        } else if s.is_empty()
            || s.starts_with("G04 ")
            || ["G01*", "M02*"].contains(&s)
            || ["%FS", "%MO", "%TF.", "%TA.", "%TO.", "%TD", "%LPD"]
                .iter()
                .any(|prefix| s.starts_with(prefix))
        {
        } else {
            return Err(format!("unsupported Gerber command: {s}").into());
        }
    }
    if region.is_some() {
        return Err("unterminated region".into());
    }
    Ok(result)
}

fn read_drill(text: &str) -> Result<Vec<(Point, f64)>, Box<dyn Error>> {
    if !text.lines().any(|s| s == "METRIC") || !text.lines().any(|s| s == "G90") {
        return Err("drill reader requires mm, absolute, decimal coordinates".into());
    }
    let mut tools = BTreeMap::new();
    let mut tool = 0;
    let mut holes = vec![];
    for s in text.lines() {
        if let Some(s) = s.strip_prefix('T') {
            if let Some((id, d)) = s.split_once('C') {
                tools.insert(id.parse::<usize>()?, d.parse::<f64>()?);
            } else {
                tool = s.parse()?;
            }
        } else if s.starts_with('X') {
            holes.push((
                xy(s, 1.0)?,
                *tools.get(&tool).ok_or("undefined drill tool")?,
            ));
        } else if s.starts_with(';') || ["M48", "METRIC", "%", "G90", "M71", "M30", ""].contains(&s)
        {
        } else {
            return Err(format!("unsupported drill command: {s}").into());
        }
    }
    Ok(holes)
}

fn svg_primitives(items: &[Primitive], color: &str) -> String {
    let mut out = String::new();
    for item in items {
        match item {
        Primitive::Flash(p,a)=> {
            if a.shape=='C' {out.push_str(&format!("<circle cx='{}' cy='{}' r='{}' fill='{color}'/>",p[0],p[1],a.size[0]/2.0));}
            else {out.push_str(&format!("<rect x='{}' y='{}' width='{}' height='{}' fill='{color}'/>",p[0]-a.size[0]/2.0,p[1]-a.size[1]/2.0,a.size[0],a.size[1]));}
        }
        Primitive::Line(a,b,w)=>out.push_str(&format!("<path d='M{},{} L{},{}' stroke='{color}' stroke-width='{w}' stroke-linecap='round' fill='none'/>",a[0],a[1],b[0],b[1])),
        Primitive::Region(rings)=>{
            let path=rings.iter().map(|r|format!("M{} Z",r.iter().map(|p|format!("{},{}",p[0],p[1])).collect::<Vec<_>>().join(" L"))).collect::<Vec<_>>().join(" ");
            out.push_str(&format!("<path d='{path}' fill='{color}' fill-rule='evenodd'/ >").replace("/ >","/>"));
        }
    }
    }
    out
}

pub fn export(
    b: &Board,
    r: &Routes,
    p: &Shapes,
    out: &Path,
    log: &mut String,
) -> Result<(), Box<dyn Error>> {
    let t = Instant::now();
    for (file, text) in [
        ("led-F_Cu.gbr", copper(b, r, 0, None)),
        ("led-B_Cu.gbr", copper(b, r, 1, Some(p))),
        ("led-Edge_Cuts.gbr", edges(b)),
        ("led.drl", drill_file(b, r)),
    ] {
        fs::write(out.join(file), text)?;
    }
    // The SVG gets every copper, edge and drill point by reading files back, never from the board structures.
    let front = read_gerber(&fs::read_to_string(out.join("led-F_Cu.gbr"))?)?;
    let bottom = read_gerber(&fs::read_to_string(out.join("led-B_Cu.gbr"))?)?;
    let edge = read_gerber(&fs::read_to_string(out.join("led-Edge_Cuts.gbr"))?)?;
    let drill = read_drill(&fs::read_to_string(out.join("led.drl"))?)?;
    let flashes = front
        .iter()
        .filter(|p| matches!(p, Primitive::Flash(..)))
        .count();
    let tracks = front
        .iter()
        .filter(|p| matches!(p, Primitive::Line(..)))
        .count();
    assert_eq!(
        flashes,
        b.pads.iter().filter(|p| p.layers & 1 != 0).count() + r.vias.len()
    );
    assert_eq!(tracks, r.tracks.iter().filter(|t| t.layer == 0).count());
    assert_eq!(edge.len(), 4);
    assert_eq!(drill.len(), drills(b, r).len());
    for &(p, d) in &drill {
        assert!(drills(b, r)
            .iter()
            .any(|&(q, e)| distance(p, q) < 1e-6 && (d - e).abs() < 1e-6));
    }
    let regions: Shapes = bottom
        .iter()
        .filter_map(|p| {
            if let Primitive::Region(s) = p {
                Some(s.clone())
            } else {
                None
            }
        })
        .collect();
    assert!((pour::area(&regions) - pour::area(p)).abs() < 1e-4);
    let mut svg=String::from("<svg xmlns='http://www.w3.org/2000/svg' width='1200' height='540' viewBox='0 0 72 32'><rect width='72' height='32' fill='#101923'/><g fill='#f1f5f9' font-family='sans-serif'><text x='3' y='3' font-size='1.6'>F_Cu + Edge_Cuts + Excellon</text><text x='39' y='3' font-size='1.6'>B_Cu: GND pour + thermal reliefs</text></g>");
    for (x, items, color) in [(18.0, &front, "#ef9c46"), (54.0, &bottom, "#58a8c2")] {
        svg.push_str(&format!("<g transform='translate({x},16) scale(1,-1)'><rect x='-15' y='-10' width='30' height='20' fill='#23343f'/ >").replace("/ >","/>"));
        svg.push_str(&svg_primitives(items, color));
        svg.push_str(&svg_primitives(&edge, "#f8fafc"));
        for &(p, d) in &drill {
            svg.push_str(&format!("<circle cx='{}' cy='{}' r='{}' fill='#101923' stroke='#ea70d8' stroke-width='0.05'/>",p[0],p[1],d/2.0));
        }
        svg.push_str("</g>");
    }
    svg.push_str("<g fill='#d4dde7' font-family='sans-serif' font-size='1.1'><text x='3' y='29'>30 x 20 mm. Origin centre. +y up. Pink rims: drill diameters.</text><text x='39' y='29'>Top view through board; bottom is not mirrored.</text></g></svg>");
    fs::write(out.join("gerber-preview.svg"), svg)?;
    crate::report(log,format!("fab: Gerber X2 mm 4.6 F_Cu flashes={flashes} draws={tracks}; Edge_Cuts lines={}; Excellon tools={} holes={}; B_Cu regions={} polygon_area_roundtrip_delta={:.9} mm2 time_ms={:.3}",edge.len(),drill.iter().map(|(_,d)|(d*1e6).round() as i64).collect::<std::collections::BTreeSet<_>>().len(),drill.len(),regions.len(),(pour::area(&regions)-pour::area(p)).abs(),t.elapsed().as_secs_f64()*1000.0));
    crate::report(log,"fab preview: gerber-preview.svg reads led-F_Cu.gbr + led-Edge_Cuts.gbr + led.drl; right panel also reads led-B_Cu.gbr".into());
    Ok(())
}

pub fn generated_preview(b: &Board, r: &Routes, out: &Path) -> std::io::Result<()> {
    let mut svg=String::from("<svg xmlns='http://www.w3.org/2000/svg' width='1000' height='840' viewBox='-27 -23 54 46'><rect x='-27' y='-23' width='54' height='46' fill='#101923'/><g transform='scale(1,-1)'><rect x='-25' y='-20' width='50' height='40' fill='#23343f' stroke='#eee' stroke-width='.08'/>");
    for z in (0..2).rev() {
        for t in &r.tracks {
            if t.layer == z {
                svg.push_str(&svg_primitives(
                    &[Primitive::Line(t.a, t.b, t.width)],
                    if z == 0 { "#ef9c46" } else { "#58a8c2" },
                ));
            }
        }
    }
    for p in &b.pads {
        let color = if p.net != usize::MAX && !r.routed[p.net] {
            "#fa6677"
        } else {
            "#bcc6cc"
        };
        svg.push_str(&svg_primitives(
            &[Primitive::Flash(
                p.at,
                Aperture {
                    shape: if p.round { 'C' } else { 'R' },
                    size: p.size,
                },
            )],
            color,
        ));
    }
    for v in &r.vias {
        svg.push_str(&format!(
            "<circle cx='{}' cy='{}' r='.3' fill='none' stroke='#fff' stroke-width='.08'/>",
            v.at[0], v.at[1]
        ));
    }
    svg.push_str("</g><text x='-25' y='22' fill='#eee' font-size='1.2'>30 parts / 40 nets. Red pads: unrouted nets. Orange top; blue bottom.</text></svg>");
    fs::write(out.join("generated-router.svg"), svg)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reader_handles_negative_coordinates_regions_and_rejects_unknown_commands() {
        let mut s = header("Copper,L1,Top");
        s.push_str("%ADD10C,0.2*%\nD10*\nX-1234567Y2000000D03*\nG36*\nX0Y0D02*\nX1000000Y0D01*\nX1000000Y1000000D01*\nX0Y0D01*\nG37*\nM02*\n");
        let p = read_gerber(&s).unwrap();
        assert_eq!(p.len(), 2);
        if let Primitive::Flash(at, _) = p[0] {
            assert_eq!(at, [-1.234567, 2.0]);
        } else {
            panic!("expected flash");
        }
        assert!(read_gerber(&(s + "G02*\n")).is_err());
        assert!(
            read_drill("M48\nMETRIC\nT01C0.3\n%\nG90\nT01\nX-1.234567Y2.000000\nM30\n").is_ok()
        );
    }
}
