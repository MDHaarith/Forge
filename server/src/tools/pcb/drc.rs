use super::{place, pour, route, Layer};
use crate::jsfmt::{number, round};
use route::{distance, segment_distance, segment_segment, Point};
use serde_json::Value;
use std::collections::BTreeMap;

// Saved circuit coordinates are rounded to six decimal places in mm.
const EPS: f64 = 1e-6;

enum Shape {
    Disk(Point, f64),
    Stroke(Point, Point, f64),
    Region(pour::Shapes),
}
impl Shape {
    fn bounds(&self) -> [f64; 4] {
        match self {
            Self::Disk(p, r) => [p[0] - r, p[1] - r, p[0] + r, p[1] + r],
            Self::Stroke(a, b, r) => [a[0].min(b[0]) - r, a[1].min(b[1]) - r,
                a[0].max(b[0]) + r, a[1].max(b[1]) + r],
            Self::Region(shapes) => shapes.iter().flatten().flatten().fold(
                [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |mut v, p| {
                    v[0] = v[0].min(p[0]); v[1] = v[1].min(p[1]);
                    v[2] = v[2].max(p[0]); v[3] = v[3].max(p[1]); v
                }),
        }
    }
    fn contains(&self, p: Point) -> bool {
        match self {
            Self::Disk(at, radius) => distance(p, *at) <= radius + EPS,
            Self::Stroke(a, b, radius) => segment_distance(p, *a, *b) <= radius + EPS,
            Self::Region(shapes) => shapes.iter().any(|rings| {
                place::inside(p, &rings[0]) && !rings[1..].iter().any(|r| place::inside(p, r))
            }),
        }
    }
    fn edges(&self) -> Vec<(Point, Point)> {
        match self {
            Self::Region(shapes) => shapes
                .iter()
                .flatten()
                .flat_map(|ring| {
                    ring.iter()
                        .zip(ring.iter().cycle().skip(1))
                        .take(ring.len())
                        .map(|(a, b)| (*a, *b))
                })
                .collect(),
            _ => vec![],
        }
    }
    fn gap(&self, other: &Self) -> f64 {
        match (self, other) {
            (Self::Disk(a, r), Self::Disk(b, s)) => distance(*a, *b) - r - s,
            (Self::Disk(p, r), Self::Stroke(a, b, s)) => segment_distance(*p, *a, *b) - r - s,
            (Self::Stroke(a, b, r), Self::Stroke(c, d, s)) => {
                segment_segment(*a, *b, *c, *d) - r - s
            }
            (Self::Disk(p, r), Self::Region(_)) => {
                let d = other
                    .edges()
                    .iter()
                    .map(|(a, b)| segment_distance(*p, *a, *b))
                    .fold(f64::INFINITY, f64::min);
                if other.contains(*p) {
                    -d - r
                } else {
                    d - r
                }
            }
            (Self::Stroke(a, b, r), Self::Region(_)) => {
                let d = other
                    .edges()
                    .iter()
                    .map(|(c, d)| segment_segment(*a, *b, *c, *d))
                    .fold(f64::INFINITY, f64::min);
                if other.contains(*a) || other.contains(*b) {
                    -r
                } else {
                    d - r
                }
            }
            (Self::Region(_), Self::Region(_)) => {
                let a = self.edges();
                let b = other.edges();
                if a.iter().any(|(p, _)| other.contains(*p))
                    || b.iter().any(|(p, _)| self.contains(*p))
                {
                    return 0.;
                }
                a.iter()
                    .flat_map(|(a, c)| b.iter().map(move |(d, e)| segment_segment(*a, *c, *d, *e)))
                    .fold(f64::INFINITY, f64::min)
            }
            _ => other.gap(self),
        }
    }
    fn edge_gap(&self, outline: &[Point]) -> f64 {
        let edges: Vec<_> = outline
            .iter()
            .zip(outline.iter().cycle().skip(1))
            .take(outline.len())
            .collect();
        match self {
            Self::Disk(p, r) => {
                let d = edges
                    .iter()
                    .map(|(a, b)| segment_distance(*p, **a, **b))
                    .fold(f64::INFINITY, f64::min);
                if place::inside(*p, outline) {
                    d - r
                } else {
                    -d - r
                }
            }
            Self::Stroke(a, b, r) => {
                let d = edges
                    .iter()
                    .map(|(c, d)| segment_segment(*a, *b, **c, **d))
                    .fold(f64::INFINITY, f64::min);
                if place::inside(*a, outline) && place::inside(*b, outline) {
                    d - r
                } else {
                    -d - r
                }
            }
            Self::Region(_) => self
                .edges()
                .iter()
                .map(|(a, b)| {
                    let d = edges
                        .iter()
                        .map(|(c, d)| segment_segment(*a, *b, **c, **d))
                        .fold(f64::INFINITY, f64::min);
                    if place::inside(*a, outline) && place::inside(*b, outline) {
                        d
                    } else {
                        -d
                    }
                })
                .fold(f64::INFINITY, f64::min),
        }
    }
}

struct SpatialIndex {
    buckets: BTreeMap<(i64, i64), Vec<usize>>,
    large: Vec<usize>,
}
impl SpatialIndex {
    fn cells(bounds: [f64; 4]) -> Option<[i64; 4]> {
        let cells = bounds.map(|v| (v / 4.).floor() as i64);
        let area = (cells[2] as i128 - cells[0] as i128 + 1).checked_mul(cells[3] as i128 - cells[1] as i128 + 1)?;
        (area > 0 && area <= 4096).then_some(cells)
    }
    fn insert(&mut self, i: usize, bounds: [f64; 4]) {
        if let Some([x0, y0, x1, y1]) = Self::cells(bounds) {
            for y in y0..=y1 { for x in x0..=x1 { self.buckets.entry((x, y)).or_default().push(i); } }
        } else { self.large.push(i); }
    }
    fn nearby(&self, bounds: [f64; 4], end: usize) -> Vec<usize> {
        let Some([x0, y0, x1, y1]) = Self::cells(bounds) else { return (0..end).collect(); };
        let mut found = self.large.clone();
        for y in y0..=y1 { for x in x0..=x1 {
            if let Some(items) = self.buckets.get(&(x, y)) { found.extend_from_slice(items); }
        } }
        found.sort_unstable(); found.dedup(); found
    }
}

struct Item {
    name: String,
    net: Option<String>,
    layers: u8,
    shape: Shape,
    hole: bool,
}
fn num(v: &Value, key: &str) -> Result<f64, String> {
    v[key]
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("invalid circuit field {key}"))
}
fn positive(v: &Value, key: &str) -> Result<f64, String> {
    let n = num(v, key)?;
    if n <= 0. {
        return Err(format!("invalid circuit field {key}"));
    }
    Ok(n)
}
fn point(v: &Value) -> Result<Point, String> {
    Ok([num(v, "x")?, num(v, "y")?])
}
fn layer(v: &Value) -> Result<u8, String> {
    match v.as_str() {
        Some("top") => Ok(1),
        Some("bottom") => Ok(2),
        Some("inner1") => Ok(4),
        Some("inner2") => Ok(8),
        _ => Err("invalid circuit layer".into()),
    }
}
fn layers(v: &Value) -> Result<u8, String> {
    if let Some(ls) = v.get("layers").and_then(Value::as_array) {
        if ls.is_empty() {
            return Err("invalid circuit layers".into());
        }
        ls.iter().try_fold(0, |mask, l| Ok(mask | layer(l)?))
    } else {
        layer(&v["layer"])
    }
}
fn ring(v: &Value) -> Result<Vec<Point>, String> {
    let points: Vec<_> = v
        .as_array()
        .ok_or("invalid circuit polygon")?
        .iter()
        .map(point)
        .collect::<Result<_, _>>()?;
    place::validate_outline(&points)?;
    Ok(points)
}
fn fmt(n: f64) -> String {
    number(round(n * 1e6) / 1e6)
}
fn root(parents: &[usize], mut i: usize) -> usize {
    while parents[i] != i {
        i = parents[i];
    }
    i
}

/// Check the saved copper, not the router's claims: edited or deleted tracks must change the report.
pub(super) fn check(spec: &str, circuit: &str) -> Result<String, String> {
    let spec = super::parse(spec)?;
    let footprints = spec.footprints()?;
    let nets = spec.netlist(&footprints)?;
    let value: Value = serde_json::from_str(circuit).map_err(|e| e.to_string())?;
    let elements = value.as_array().ok_or("circuit must be an array")?;
    let board = elements
        .iter()
        .find(|e| e["type"] == "pcb_board")
        .ok_or("circuit has no pcb_board")?;
    let outline = ring(&board["outline"])?;
    let rules = &spec.rules;
    let clearance = |net: Option<&str>| net.map(|n| spec.clearance(n)).unwrap_or(rules.clearance);
    let width = |net: Option<&str>| {
        nets.iter()
            .find(|n| Some(n.name.as_str()) == net)
            .map(|n| n.trace)
            .unwrap_or(rules.trace)
    };
    let mut items = Vec::new();
    let mut faults = Vec::new();
    let mut pads = BTreeMap::new();
    let mut smd = Vec::new();
    let mut vias = Vec::new();
    let mut parts: Vec<_> = footprints
        .into_iter()
        .enumerate()
        .map(|(part, footprint)| super::PlacedPart {
            part,
            footprint,
            at: [0.; 2],
            rotation: 0.,
            side: Layer::Top,
        })
        .collect();
    let mut seen_parts = vec![false; parts.len()];
    for e in elements {
        let net = e["net"].as_str().map(str::to_owned);
        let mut add = |name, shape, mask, hole| {
            let i = items.len();
            items.push(Item {
                name,
                shape,
                layers: mask,
                hole,
                net: net.clone(),
            });
            i
        };
        match e["type"].as_str() {
            Some("pcb_smtpad") => {
                let name = e["pcb_smtpad_id"]
                    .as_str()
                    .ok_or("invalid pad id")?
                    .to_owned();
                let p = route::Pad {
                    name: name.clone(),
                    at: point(e)?,
                    size: [positive(e, "width")?, positive(e, "height")?],
                    rotation: num(e, "rotation")?,
                    round: false,
                    radius: e.get("rect_border_radius").map(|_| num(e, "rect_border_radius")).transpose()?.unwrap_or(0.),
                    layers: layers(e)?,
                    drill: None,
                    plated: true,
                    net: usize::MAX,
                };
                let corners = pour::rect([0.; 2], p.size)
                    .into_iter()
                    .map(|q| p.point(q))
                    .collect();
                let i = add(
                    name.clone(),
                    Shape::Region(vec![vec![corners]]),
                    p.layers,
                    false,
                );
                pads.insert(name, i);
                smd.push((p, i));
            }
            Some("pcb_plated_hole" | "pcb_via") => {
                let via = e["type"] == "pcb_via";
                let name = if via {
                    format!("via {}", net.as_deref().unwrap_or("unconnected"))
                } else {
                    e["pcb_plated_hole_id"]
                        .as_str()
                        .ok_or("invalid pad id")?
                        .to_owned()
                };
                let at = point(e)?;
                let is_oval = e.get("shape").and_then(Value::as_str) == Some("oval");
                let (diameter, drill, shape) = if is_oval {
                    let ow = positive(e, "outer_width")?;
                    let oh = positive(e, "outer_height")?;
                    let hw = positive(e, "hole_width")?;
                    let hh = positive(e, "hole_height")?;
                    let rot = num(e, "rotation").unwrap_or(0.);
                    let rad = rot.to_radians();
                    let cos = rad.cos();
                    let sin = rad.sin();
                    let (a, b, r) = if ow >= oh {
                        let half = (ow - oh) / 2.;
                        (
                            [at[0] - half * cos, at[1] - half * sin],
                            [at[0] + half * cos, at[1] + half * sin],
                            oh / 2.,
                        )
                    } else {
                        let half = (oh - ow) / 2.;
                        (
                            [at[0] + half * sin, at[1] - half * cos],
                            [at[0] - half * sin, at[1] + half * cos],
                            ow / 2.,
                        )
                    };
                    (ow.min(oh), hw.min(hh), Shape::Stroke(a, b, r))
                } else {
                    let d = positive(e, "outer_diameter")?;
                    let dr = positive(e, "hole_diameter")?;
                    (d, dr, Shape::Disk(at, d / 2.))
                };
                let i = add(name.clone(), shape, layers(e)?, false);
                let annulus = (diameter - drill) / 2.;
                let minimum = (rules.via[0] - rules.via[1]) / 2.;
                if annulus + EPS < minimum {
                    faults.push(format!(
                        "annular ring: {name} is {} mm, needs {} mm",
                        fmt(annulus),
                        fmt(minimum)
                    ));
                }
                if drill + EPS < rules.via[1] {
                    faults.push(format!(
                        "drill size: {name} is {} mm, needs {} mm",
                        fmt(drill),
                        fmt(rules.via[1])
                    ));
                }
                if via {
                    vias.push((i, at, diameter / 2.));
                } else {
                    pads.insert(name, i);
                }
            }
            Some("pcb_hole") => {
                let name = e["pcb_hole_id"].as_str().unwrap_or("hole").to_owned();
                let at = point(e)?;
                let drill = positive(e, "hole_diameter")?;
                if drill + EPS < rules.via[1] {
                    faults.push(format!(
                        "drill size: {name} is {} mm, needs {} mm",
                        fmt(drill),
                        fmt(rules.via[1])
                    ));
                }
                add(
                    name,
                    Shape::Disk(at, drill / 2.),
                    (1 << spec.board.layers) - 1,
                    true,
                );
            }
            Some("pcb_trace") => {
                let points = e["route"].as_array().ok_or("invalid trace route")?;
                if points.len() < 2 {
                    return Err("invalid trace route".into());
                }
                let trace_width = positive(e, "width")?;
                let mut narrowest = trace_width;
                let name = format!("track {}", net.as_deref().unwrap_or("unconnected"));
                for pair in points.windows(2) {
                    let w = if pair[0].get("width").is_some() {
                        positive(&pair[0], "width")?
                    } else {
                        trace_width
                    };
                    narrowest = narrowest.min(w);
                    let mask = layer(&pair[0]["layer"])?;
                    if mask != layer(&pair[1]["layer"])? {
                        return Err("trace changes layer without a via".into());
                    }
                    add(
                        name.clone(),
                        Shape::Stroke(point(&pair[0])?, point(&pair[1])?, w / 2.),
                        mask,
                        false,
                    );
                }
                let minimum = width(net.as_deref());
                if narrowest + EPS < minimum {
                    faults.push(format!(
                        "track width: {name} is {} mm, needs {} mm",
                        fmt(narrowest),
                        fmt(minimum)
                    ));
                }
            }
            Some("pcb_copper_pour") => {
                let brep = &e["brep_shape"];
                let mut rings = vec![ring(&brep["outer_ring"]["vertices"])?];
                for r in brep["inner_rings"].as_array().ok_or("invalid pour rings")? {
                    rings.push(ring(&r["vertices"])?);
                }
                add(
                    format!("pour {}", net.as_deref().unwrap_or("unconnected")),
                    Shape::Region(vec![rings]),
                    layers(e)?,
                    false,
                );
            }
            Some("pcb_component") => {
                let p = parts
                    .iter_mut()
                    .find(|p| e["pcb_component_id"] == format!("pcb_component_{}", p.part))
                    .ok_or("invalid component id")?;
                seen_parts[p.part] = true;
                p.at = point(&e["center"])?;
                p.rotation = num(e, "rotation")?;
                p.side = match layer(&e["layer"])? {
                    1 => Layer::Top,
                    2 => Layer::Bottom,
                    _ => return Err("invalid component layer".into()),
                };
            }
            Some("pcb_board" | "source_component") => {}
            _ => return Err("invalid circuit element type".into()),
        }
    }
    if seen_parts.iter().any(|seen| !seen) {
        return Err("circuit is missing a pcb_component".into());
    }
    for (i, h) in spec.holes.iter().enumerate() {
        if h.d + EPS < rules.via[1] {
            faults.push(format!(
                "drill size: hole {} is {} mm, needs {} mm",
                i + 1,
                fmt(h.d),
                fmt(rules.via[1])
            ));
        }
        items.push(Item {
            name: format!("hole {}", i + 1),
            net: None,
            layers: (1 << spec.board.layers) - 1,
            shape: Shape::Disk(h.at, h.d / 2.),
            hole: true,
        });
    }
    for (i, at, radius) in vias {
        for (p, pad) in &smd {
            if rules.via_in_pad && items[i].net.is_some() && items[i].net == items[*pad].net {
                continue;
            }
            let q = p.local(at);
            if route::rect_distance(q, [0.; 2], p.size) <= radius + EPS {
                faults.push(format!("via in pad: {} in {}", items[i].name, p.name));
            }
        }
    }
    let mut parents: Vec<_> = (0..items.len()).collect();
    let mut index = SpatialIndex { buckets: BTreeMap::new(), large: vec![] };
    let max_clearance = spec.netclasses.values().filter_map(|c| c.clearance).fold(rules.clearance, f64::max) + EPS;
    let bounds: Vec<_> = items.iter().map(|i| i.shape.bounds()).collect();
    for (i, a) in items.iter().enumerate() {
        if !a.hole {
            let gap = a.shape.edge_gap(&outline);
            if gap + EPS < rules.edge {
                faults.push(format!(
                    "edge: {} is {} mm from the board edge, needs {} mm",
                    a.name,
                    fmt(gap),
                    fmt(rules.edge)
                ));
            }
        }
        let [x0, y0, x1, y1] = bounds[i];
        for j in index.nearby([x0 - max_clearance, y0 - max_clearance, x1 + max_clearance, y1 + max_clearance], i) {
            let b = &items[j];
            if bounds[j][0] > x1 + max_clearance || bounds[j][1] > y1 + max_clearance
                || bounds[j][2] < x0 - max_clearance || bounds[j][3] < y0 - max_clearance { continue; }
            if a.layers & b.layers == 0 || (a.hole && b.hole) {
                continue;
            }
            let same = a.net.is_some() && a.net == b.net;
            let gap = a.shape.gap(&b.shape);
            if same {
                if gap <= EPS {
                    let ra = root(&parents, i);
                    let rb = root(&parents, j);
                    parents[ra] = rb;
                }
            } else {
                let same_comp = !a.name.is_empty()
                    && !b.name.is_empty()
                    && a.name.split('.').next() == b.name.split('.').next()
                    && (a.net == b.net || a.net.is_none() || b.net.is_none());
                if same_comp && gap <= EPS {
                    continue;
                }
                let rule = if a.hole {
                    clearance(b.net.as_deref())
                } else if b.hole {
                    clearance(a.net.as_deref())
                } else {
                    clearance(a.net.as_deref()).max(clearance(b.net.as_deref()))
                };
                if gap + EPS < rule {
                    faults.push(format!(
                        "clearance: {} to {} is {} mm, needs {} mm",
                        b.name,
                        a.name,
                        fmt(gap),
                        fmt(rule)
                    ));
                }
            }
        }
        index.insert(i, bounds[i]);
    }
    for net in &nets {
        let names: Vec<_> = net
            .pins
            .iter()
            .map(|pin| format!("{}.{}", spec.parts[pin.part].reference, pin.number))
            .collect();
        if let Some(first) = names.first() {
            for name in &names[1..] {
                if pads
                    .get(first)
                    .zip(pads.get(name))
                    .is_none_or(|(&a, &b)| root(&parents, a) != root(&parents, b))
                {
                    faults.push(format!(
                        "unrouted: net {} ({} to {})",
                        net.name, first, name
                    ));
                }
            }
        }
    }
    for (i, a) in parts.iter().enumerate() {
        for b in &parts[..i] {
            if place::overlap(a, b) {
                faults.push(format!(
                    "parts overlap: {} and {}",
                    spec.parts[b.part].reference, spec.parts[a.part].reference
                ));
            }
        }
    }
    faults.sort();
    let summary = if faults.is_empty() {
        "No DRC errors.".into()
    } else {
        format!("{} DRC error(s).", faults.len())
    };
    let mut warnings = Vec::new();
    for net in &nets {
        if let [pin] = net.pins.as_slice() {
            warnings.push(format!("erc: net {} has one pin ({}.{})", net.name, spec.parts[pin.part].reference, pin.number));
        }
    }
    for p in &parts {
        let part = &spec.parts[p.part];
        let connected: Vec<_> = p.footprint.pads.iter().map(|pad| {
            nets.iter().find(|net| net.pins.iter().any(|pin| pin.part == p.part && pin.number == pad.number))
        }).collect();
        if connected.iter().all(Option::is_none) {
            if part.kind != super::Kind::Antenna {
                warnings.push(format!("erc: {} has no connected pins", part.reference));
            }
        } else if p.footprint.pads.len() == 2 && matches!(part.kind,
            super::Kind::Resistor | super::Kind::Capacitor | super::Kind::Inductor |
            super::Kind::Led | super::Kind::Diode | super::Kind::Crystal)
        {
            for (pad, net) in p.footprint.pads.iter().zip(&connected) {
                if net.is_none() {
                    warnings.push(format!("erc: {}.{} is not connected", part.reference, pad.number));
                }
            }
            if let (Some(a), Some(b)) = (connected[0], connected[1]) {
                if a.name == b.name {
                    warnings.push(format!("erc: {} has both pins on net {}", part.reference, a.name));
                }
            }
        }
    }
    warnings.sort();
    let count = warnings.len();
    faults.extend(warnings);
    if rules.via_in_pad {
        faults.push("Filled/capped vias-in-pad are required.".into());
    }
    if count > 0 {
        faults.push(format!("{count} ERC warning(s)."));
    }
    faults.push(summary);
    Ok(faults.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SPEC: &str = r#"{"board":{"width":30,"height":20},"parts":[
        {"ref":"R1","kind":"resistor","footprint":"0402","at":[-5,0]},
        {"ref":"R2","kind":"resistor","footprint":"0402","at":[5,0]}],
        "nets":{"N":["R1.1","R2.1"]},"holes":[{"at":[0,5],"d":2}]}"#;
    fn circuit(spec: &str) -> Value {
        let prepared = super::super::prepare(spec).unwrap();
        let board = route::Board::from_prepared(&prepared).unwrap();
        let routed = route::route(&board);
        super::super::circuit(&prepared, &board, &routed, &vec![])
    }
    #[test]
    fn spatial_index_keeps_every_close_pair_across_bucket_edges() {
        let shapes: Vec<_> = (0..100).map(|i| {
            let p = [(i * 37 % 101) as f64 / 5. - 10., (i * 61 % 103) as f64 / 5. - 10.];
            match i % 3 {
                0 => Shape::Disk(p, 0.3),
                1 => Shape::Stroke(p, [p[0] + 8.1, p[1] + 4.2], 0.15),
                _ => Shape::Region(vec![vec![pour::rect(p, [1.2, 0.8])]]),
            }
        }).collect();
        let mut index = SpatialIndex { buckets: BTreeMap::new(), large: vec![] };
        for (i, shape) in shapes.iter().enumerate() {
            let [x0, y0, x1, y1] = shape.bounds();
            let found = index.nearby([x0 - 0.8, y0 - 0.8, x1 + 0.8, y1 + 0.8], i);
            for (j, other) in shapes[..i].iter().enumerate() {
                if shape.gap(other) <= 0.8 { assert!(found.contains(&j), "missed {j} to {i}"); }
            }
            index.insert(i, shape.bounds());
        }
        assert!(SpatialIndex::cells([-1e100, -1e100, 1e100, 1e100]).is_none());
    }

    #[test]
    fn deleted_copper_breaks_connectivity_and_reports_are_sorted() {
        let mut copper = circuit(SPEC);
        assert_eq!(check(SPEC, &copper.to_string()).unwrap(), "erc: R1.2 is not connected\nerc: R2.2 is not connected\n2 ERC warning(s).\nNo DRC errors.");
        copper
            .as_array_mut()
            .unwrap()
            .retain(|e| e["type"] != "pcb_trace");
        assert_eq!(
            check(SPEC, &copper.to_string()).unwrap(),
            "unrouted: net N (R1.1 to R2.1)\nerc: R1.2 is not connected\nerc: R2.2 is not connected\n2 ERC warning(s).\n1 DRC error(s)."
        );
        let rows = copper.as_array_mut().unwrap();
        let pad = rows
            .iter()
            .find(|e| e["pcb_smtpad_id"] == "R1.1")
            .unwrap()
            .clone();
        rows.push(
            json!({"type":"pcb_via","pcb_via_id":"bad","x":pad["x"],"y":pad["y"],
            "outer_diameter":0.2,"hole_diameter":0.1,"layers":["top","bottom"],"net":"N"}),
        );
        rows.iter_mut()
            .find(|e| e["pcb_component_id"] == "pcb_component_1" && e["type"] == "pcb_component")
            .unwrap()["center"] = json!({"x":-5,"y":0});
        rows.iter_mut()
            .find(|e| e["pcb_smtpad_id"] == "R2.2")
            .unwrap()["x"] = json!(14.9);
        rows.push(
            json!({"type":"pcb_trace","pcb_trace_id":"hole_track","width":0.2,"net":"X",
            "route":[{"x":-2,"y":5,"layer":"top"},{"x":2,"y":5,"layer":"top"}]}),
        );
        let report = check(SPEC, &copper.to_string()).unwrap();
        for expected in [
            "annular ring: via N is 0.05 mm, needs 0.15 mm",
            "drill size: via N is 0.1 mm, needs 0.3 mm",
            "via in pad: via N in R1.1",
            "parts overlap: R1 and R2",
            "edge: R2.2",
            "clearance: track X to hole 1",
        ] {
            assert!(report.contains(expected), "missing {expected}: {report}");
        }
        let lines: Vec<_> = report.lines().collect();
        let erc = lines.iter().position(|l| l.starts_with("erc:")).unwrap();
        assert!(lines[..erc].windows(2).all(|p| p[0] <= p[1]));
        assert!(lines[erc..lines.len()-2].windows(2).all(|p| p[0] <= p[1]));
        assert_eq!(lines[lines.len()-2], "2 ERC warning(s).");
        assert_eq!(
            lines.last().unwrap(),
            &format!("{erc} DRC error(s).")
        );
        assert_eq!(report, check(SPEC, &copper.to_string()).unwrap());
    }
    #[test]
    fn netclass_rules_apply_to_saved_copper_and_router() {
        let mut spec: Value = serde_json::from_str(SPEC).unwrap();
        spec["netclasses"] = json!({"power":{"nets":["N"],"trace":0.4,"clearance":0.3}});
        let spec = spec.to_string();
        let mut copper = circuit(&spec);
        assert_eq!(check(&spec, &copper.to_string()).unwrap(), "erc: R1.2 is not connected\nerc: R2.2 is not connected\n2 ERC warning(s).\nNo DRC errors.");
        let rows = copper.as_array_mut().unwrap();
        let pad = rows
            .iter_mut()
            .find(|e| e["pcb_smtpad_id"] == "R1.2")
            .unwrap();
        pad["x"] = json!(-5.485 + 0.59 + 0.25);
        let trace = rows.iter_mut().find(|e| e["type"] == "pcb_trace").unwrap();
        trace["width"] = json!(0.2);
        for point in trace["route"].as_array_mut().unwrap() {
            point["width"] = json!(0.2);
        }
        let report = check(&spec, &copper.to_string()).unwrap();
        assert!(
            report.contains("clearance: R1.1 to R1.2 is 0.25 mm, needs 0.3 mm"),
            "{report}"
        );
        assert!(
            report.contains("track width: track N is 0.2 mm, needs 0.4 mm"),
            "{report}"
        );
    }
    #[test]
    fn changed_edge_rules_check_saved_placements() {
        let copper = circuit(SPEC);
        let mut spec: Value = serde_json::from_str(SPEC).unwrap();
        spec["rules"] = json!({"edge":9.5});
        let report = check(&spec.to_string(), &copper.to_string()).unwrap();
        assert!(report.contains("edge: R1.1"), "{report}");
        assert!(!report.contains("unrouted:"), "{report}");
    }
    #[test]
    fn malformed_geometry_is_rejected() {
        let mut copper = circuit(SPEC);
        let rows = copper.as_array_mut().unwrap();
        rows.iter_mut().find(|e| e["type"] == "pcb_smtpad").unwrap()["width"] = json!(-1);
        assert_eq!(
            check(SPEC, &copper.to_string()).unwrap_err(),
            "invalid circuit field width"
        );
    }

    #[test]
    fn electrical_warnings_use_expanded_pins_and_do_not_count_as_faults() {
        let spec = json!({"board":{"width":30,"height":20},"parts":[
            {"ref":"D1","kind":"diode","footprint":"0603","at":[-5,0],"pins":{"1":"S","2":"S"}},
            {"ref":"R1","kind":"resistor","footprint":"0402","at":[5,0],"pins":{"1":"ONE"}},
            {"ref":"R2","kind":"resistor","footprint":"0402","at":[0,5]},
            {"ref":"J1","kind":"header","footprint":"pinheader_1x2","at":[0,-5]}]}).to_string();
        let mut copper = circuit(&spec);
        let expected = "erc: D1 has both pins on net S\nerc: J1 has no connected pins\nerc: R1.2 is not connected\nerc: R2 has no connected pins\nerc: net ONE has one pin (R1.1)\n5 ERC warning(s).\nNo DRC errors.";
        assert_eq!(check(&spec, &copper.to_string()).unwrap(), expected);
        for e in copper.as_array_mut().unwrap() { e.as_object_mut().unwrap().remove("rect_border_radius"); }
        assert_eq!(check(&spec, &copper.to_string()).unwrap(), expected);
    }
}
