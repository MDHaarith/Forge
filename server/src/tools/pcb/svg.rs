use super::{gerber, pour, route, Kind, Layer, Pours, PreparedBoard};
use crate::jsfmt::fixed_string;
use std::{cmp::Reverse, collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque}};

fn n(v: f64) -> String {
    fixed_string(v, 6)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn path(rings: &[pour::Contour]) -> String {
    rings
        .iter()
        .map(|ring| {
            format!(
                "M{} Z",
                ring.iter()
                    .map(|p| format!("{},{}", n(p[0]), n(p[1])))
                    .collect::<Vec<_>>()
                    .join(" L")
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn line(out: &mut String, a: route::Point, b: route::Point, width: f64, color: &str) {
    out.push_str(&format!("<path d=\"M{},{} L{},{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{}\" stroke-linecap=\"round\"/>", n(a[0]), n(a[1]), n(b[0]), n(b[1]), n(width)));
}

fn circle(out: &mut String, at: route::Point, radius: f64, color: &str) {
    out.push_str(&format!(
        "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{color}\"/>",
        n(at[0]),
        n(at[1]),
        n(radius)
    ));
}

fn text(out: &mut String, at: route::Point, size: f64, label: &str) {
    out.push_str(&format!("<text x=\"{}\" y=\"{}\" font-family=\"sans-serif\" font-size=\"{}\" fill=\"#e2e8f0\">{}</text>", n(at[0]), n(at[1]), n(size), escape(label)));
}

pub(super) fn board(
    prepared: &PreparedBoard,
    b: &route::Board,
    r: &route::Routes,
    pours: &Pours,
) -> String {
    let [x0, y0, x1, y1] = prepared.spec.board.bounds();
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\"><g transform=\"scale(1,-1)\">", n(x0 - 1.), n(-y1 - 1.), n(x1 - x0 + 2.), n(y1 - y0 + 2.));
    out.push_str(&format!(
        "<path d=\"{}\" fill=\"#246c47\" stroke=\"#cbd5e1\" stroke-width=\"0.05\"/>",
        path(std::slice::from_ref(&b.outline))
    ));
    for layer in [Layer::Bottom, Layer::Inner2, Layer::Inner1, Layer::Top] {
        let color = match layer {
            Layer::Top => "#ef9c46",
            Layer::Bottom => "#58a8c2",
            _ => "#9f83d4",
        };
        for (_, _, shapes) in pours.iter().filter(|(_, l, _)| *l == layer) {
            for shape in shapes {
                out.push_str(&format!(
                    "<path d=\"{}\" fill=\"{color}\" fill-rule=\"evenodd\" opacity=\"0.35\"/>",
                    path(shape)
                ));
            }
        }
        for t in r
            .tracks
            .iter()
            .chain(b.fixed.iter().map(|f| &f.track))
            .filter(|t| t.layer == layer.index())
        {
            line(&mut out, t.a, t.b, t.width, color);
        }
        for t in r.teardrops.iter().filter(|t| t.layer == layer.index()) {
            out.push_str(&format!("<path d=\"{}\" fill=\"{color}\"/>", path(std::slice::from_ref(&t.points))));
        }
    }
    for p in &b.pads {
        if !p.plated {
            continue;
        }
        if p.round {
            if (p.size[0] - p.size[1]).abs() > 1e-6 {
                let (w, h) = (p.size[0], p.size[1]);
                let rad = p.rotation.to_radians();
                let cos = rad.cos();
                let sin = rad.sin();
                let (a, b, r) = if w >= h {
                    let half = (w - h) / 2.;
                    (
                        [p.at[0] - half * cos, p.at[1] - half * sin],
                        [p.at[0] + half * cos, p.at[1] + half * sin],
                        h / 2.,
                    )
                } else {
                    let half = (h - w) / 2.;
                    (
                        [p.at[0] + half * sin, p.at[1] - half * cos],
                        [p.at[0] - half * sin, p.at[1] + half * cos],
                        w / 2.,
                    )
                };
                line(&mut out, a, b, r * 2., "#e8c66a");
            } else {
                circle(&mut out, p.at, p.size[0] / 2., "#e8c66a");
            }
        } else if p.radius > 0. {
            out.push_str(&format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" ry=\"{}\" transform=\"translate({},{}) rotate({})\" fill=\"#e8c66a\"/>",
                n(-p.size[0]/2.), n(-p.size[1]/2.), n(p.size[0]), n(p.size[1]), n(p.radius), n(p.radius), n(p.at[0]), n(p.at[1]), n(p.rotation)));
        } else {
            let ring = pour::rect([0.; 2], p.size)
                .into_iter()
                .map(|q| p.point(q))
                .collect();
            out.push_str(&format!("<path d=\"{}\" fill=\"#e8c66a\"/>", path(&[ring])));
        }
    }
    for v in &r.vias {
        circle(&mut out, v.at, b.via[0] / 2., "#e8c66a");
    }
    for p in &prepared.parts {
        let silk: Vec<_> = p
            .footprint
            .silkscreen
            .iter()
            .map(|q| p.point([q[0], -q[1]]))
            .collect();
        for pair in silk.windows(2) {
            line(&mut out, pair[0], pair[1], 0.15, "#f8fafc");
        }
        if let (Some(a), Some(z)) = (silk.first(), silk.last()) {
            line(&mut out, *z, *a, 0.15, "#f8fafc");
        }
        let mark = p.footprint.pin1;
        circle(&mut out, p.point([mark[0], -mark[1]]), 0.15, "#f8fafc");
    }
    for hole in gerber::drills(b, r, true)
        .into_iter()
        .chain(gerber::drills(b, r, false))
    {
        if let Some([p1, p2]) = hole.slot {
            line(&mut out, p1, p2, hole.tool, "#101923");
        } else {
            circle(&mut out, hole.at, hole.tool / 2., "#101923");
        }
    }
    for v in r.vias.iter().filter(|v| v.span != (0, b.layers - 1)) {
        circle(&mut out, v.at, b.via[1] / 2., "#101923");
    }
    out.push_str("</g>");
    for p in &prepared.parts {
        text(
            &mut out,
            [p.at[0], -p.at[1]],
            1.,
            &prepared.spec.parts[p.part].reference,
        );
    }
    out.push_str("</svg>");
    out
}

pub(super) fn power_net(name: &str) -> Option<bool> {
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "GND" | "AGND" | "DGND" | "PGND" | "GNDA" | "GNDD" | "VSS" | "VSSA" => Some(true),
        "VCC" | "VDD" | "VDDA" | "VDDIO" | "VBAT" | "VBUS" | "VIN" | "VSYS" => Some(false),
        _ => {
            let voltage = upper.strip_prefix('+').unwrap_or(&upper);
            voltage.split_once('V').filter(|(a, b)| !a.is_empty()
                && a.bytes().all(|c| c.is_ascii_digit())
                && b.bytes().all(|c| c.is_ascii_digit())).map(|_| false)
        }
    }
}

const G: f64 = 2.54;
type GridPoint = [i32; 2];
type Tree = BTreeMap<GridPoint, BTreeSet<GridPoint>>;
type Occupied = BTreeMap<GridPoint, (u8, bool)>;
const STEPS: [GridPoint; 4] = [[1, 0], [-1, 0], [0, 1], [0, -1]];

fn xy(p: GridPoint) -> String { format!("{},{}", n(p[0] as f64 * G), n(p[1] as f64 * G)) }

fn horizontal(kind: Kind, pads: usize) -> bool {
    pads == 2 && matches!(kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor |
        Kind::Led | Kind::Diode | Kind::Crystal | Kind::Button | Kind::Switch)
}

fn direction(a: GridPoint, b: GridPoint) -> usize {
    if a[0] < b[0] { 0 } else if a[0] > b[0] { 1 } else if a[1] < b[1] { 2 } else { 3 }
}

fn vertex(tree: &Tree, at: GridPoint, pins: &BTreeSet<GridPoint>) -> bool {
    let neighbours = &tree[&at];
    let mask = neighbours.iter().fold(0, |m, q| m | (1 << direction(at, *q)));
    pins.contains(&at) || !matches!(mask, 3 | 12)
}

fn wire_path(start: GridPoint, tree: &Tree, blocked: &BTreeSet<GridPoint>, occupied: &Occupied,
    pins: &BTreeMap<GridPoint, Option<usize>>, net: usize, size: GridPoint) -> Option<Vec<GridPoint>>
{
    let heuristic = |p: GridPoint| tree.keys().filter(|q| !occupied.contains_key(*q))
        .map(|q| (p[0]-q[0]).abs() + (p[1]-q[1]).abs()).min().unwrap_or(0) * 10;
    let mut queue = BinaryHeap::new();
    let mut best = BTreeMap::new();
    let initial = (start, 4);
    best.insert(initial, (0, initial));
    queue.push(Reverse((heuristic(start), 0, initial)));
    while let Some(Reverse((_, cost, state @ (at, previous)))) = queue.pop() {
        if best[&state].0 != cost { continue; }
        if tree.contains_key(&at) && !occupied.contains_key(&at) {
            let mut path = vec![at];
            let mut cursor = state;
            while cursor != initial {
                cursor = best[&cursor].1;
                path.push(cursor.0);
            }
            path.reverse();
            return Some(path);
        }
        for (dir, step) in STEPS.iter().enumerate() {
            if previous < 4 && dir == previous ^ 1 { continue; }
            if occupied.contains_key(&at) && dir != previous { continue; }
            let q = [at[0]+step[0], at[1]+step[1]];
            if q[0] < 1 || q[1] < 1 || q[0] >= size[0] || q[1] >= size[1]
                || blocked.contains(&q) || pins.get(&q).is_some_and(|owner| *owner != Some(net))
            { continue; }
            if let Some(&(mask, end)) = occupied.get(&q) {
                if end || !matches!(mask, 3 | 12) || (mask == 3) == (dir < 2) { continue; }
            }
            let next = (q, dir);
            let next_cost = cost + 10 + if previous < 4 && previous != dir { 6 } else { 0 }
                + if occupied.contains_key(&q) { 10 } else { 0 };
            if best.get(&next).is_none_or(|(c, _)| next_cost < *c) {
                best.insert(next, (next_cost, state));
                queue.push(Reverse((next_cost + heuristic(q), next_cost, next)));
            }
        }
    }
    None
}

fn wire_tree(points: &[GridPoint], blocked: &BTreeSet<GridPoint>, occupied: &Occupied,
    pins: &BTreeMap<GridPoint, Option<usize>>, net: usize, size: GridPoint) -> Option<Tree>
{
    let &first = points.first()?;
    let mut tree = Tree::from([(first, BTreeSet::new())]);
    for &pin in &points[1..] {
        for pair in wire_path(pin, &tree, blocked, occupied, pins, net, size)?.windows(2) {
            tree.entry(pair[0]).or_default().insert(pair[1]);
            tree.entry(pair[1]).or_default().insert(pair[0]);
        }
    }
    if tree[&first].is_empty() {
        let q = STEPS.iter().map(|d| [first[0]+d[0], first[1]+d[1]]).find(|q|
            q[0] > 0 && q[1] > 0 && q[0] < size[0] && q[1] < size[1]
            && !blocked.contains(q) && !occupied.contains_key(q) && !pins.contains_key(q))?;
        tree.get_mut(&first).unwrap().insert(q);
        tree.insert(q, BTreeSet::from([first]));
    }
    Some(tree)
}

fn wires(out: &mut String, tree: &Tree, pins: &BTreeSet<GridPoint>, net: &str, occupied: &mut Occupied) {
    let mut used = BTreeSet::new();
    for (&at, neighbours) in tree {
        let mask = neighbours.iter().fold(0, |m, q| m | (1 << direction(at, *q)));
        let end = vertex(tree, at, pins);
        occupied.entry(at).and_modify(|(m, e)| { *m |= mask; *e |= end; }).or_insert((mask, end));
        if neighbours.len() >= 3 {
            out.push_str(&format!("<circle class=\"junction\" data-net=\"{}\" cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#7dd3fc\"/>", escape(net), n(at[0] as f64*G), n(at[1] as f64*G), n(0.4)));
        }
        if !vertex(tree, at, pins) { continue; }
        for &next in neighbours {
            let edge = if at < next { (at, next) } else { (next, at) };
            if !used.insert(edge) { continue; }
            let (mut previous, mut end) = (at, next);
            while !vertex(tree, end, pins) {
                let q = *tree[&end].iter().find(|&&q| q != previous).unwrap();
                used.insert(if end < q { (end, q) } else { (q, end) });
                previous = end;
                end = q;
            }
            out.push_str(&format!("<polyline class=\"wire\" data-net=\"{}\" points=\"{} {}\" fill=\"none\" stroke=\"#7dd3fc\" stroke-width=\"{}\"/>", escape(net), xy(at), xy(end), n(0.35)));
        }
    }
}

pub(super) fn schematic(b: &PreparedBoard) -> String {
    let mut order = Vec::new();
    let mut seen = BTreeSet::new();
    while seen.len() < b.parts.len() {
        let start = b.parts.iter().filter(|p| !seen.contains(&p.part))
            .min_by_key(|p| (Reverse(p.footprint.pads.len()), p.part)).unwrap().part;
        let mut queue = VecDeque::from([start]);
        seen.insert(start);
        while let Some(part) = queue.pop_front() {
            order.push(part);
            for net in &b.nets {
                if net.pins.iter().any(|p| p.part == part) {
                    for pin in &net.pins {
                        if seen.insert(pin.part) { queue.push_back(pin.part); }
                    }
                }
            }
        }
    }
    let label_length = b.nets.iter().map(|n| n.name.chars().count()).max().unwrap_or(0);
    let box_width = 10.max((label_length as f64 * 1.8 / G).ceil() as i32 + 4);
    let dimensions: Vec<_> = b.parts.iter().map(|p| {
        if horizontal(b.spec.parts[p.part].kind, p.footprint.pads.len()) { [6, 4] }
        else { [box_width, 2 * (p.footprint.pads.len().div_ceil(2) as i32 + 1)] }
    }).collect();
    let title_width = b.spec.parts.iter().map(|p| p.reference.chars().count()
        + p.value.as_deref().unwrap_or("").chars().count()+1).max().unwrap_or(0);
    let cell = [dimensions.iter().map(|d| d[0]).max().unwrap_or(6)
        .max((title_width as f64 * 1.4 / G).ceil() as i32) + 16,
        dimensions.iter().map(|d| d[1]).max().unwrap_or(4) + 14];
    let columns = b.parts.len().clamp(1, 3);
    let size = [cell[0]*columns as i32+8, cell[1]*b.parts.len().div_ceil(columns).max(1) as i32+8];
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"#101923\"/>", n(size[0] as f64*G), n(size[1] as f64*G));
    let mut blocked = BTreeSet::new();
    let mut pins = BTreeMap::new();
    let mut pin_left = BTreeSet::new();
    let mut endpoints = vec![Vec::new(); b.nets.len()];
    for (index, part_index) in order.into_iter().enumerate() {
        let p = b.parts.iter().find(|p| p.part == part_index).unwrap();
        let part = &b.spec.parts[part_index];
        let [w, h] = dimensions[b.parts.iter().position(|p| p.part == part_index).unwrap()];
        let [x, y] = [(index % columns) as i32*cell[0]+8, (index / columns) as i32*cell[1]+8];
        for xx in x..=x+w {
            for yy in y..=y+h { blocked.insert([xx, yy]); }
        }
        // Reserve the reference/value line as well as the symbol body.
        for xx in x..x+cell[0]-12 {
            for yy in y-3..y { blocked.insert([xx, yy]); }
        }
        out.push_str(&format!("<g class=\"part\" data-ref=\"{}\"><rect class=\"body\" data-ref=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"/>", escape(&part.reference), escape(&part.reference), n(x as f64*G), n(y as f64*G), n(w as f64*G), n(h as f64*G), if horizontal(part.kind, p.footprint.pads.len()) { "none" } else { "#ef9c46" }, n(0.4)));
        text(&mut out, [x as f64*G, (y-1) as f64*G], 2.8,
            &format!("{} {}", part.reference, part.value.as_deref().unwrap_or("")));
        let two = horizontal(part.kind, p.footprint.pads.len());
        if two {
            let symbol = match part.kind {
                Kind::Resistor => "M0,5 H3 L4,2 L6,8 L8,2 L10,8 L12,2 L13,5 H16",
                Kind::Capacitor => "M0,5 H7 M7,0 V10 M9,0 V10 M9,5 H16",
                Kind::Diode | Kind::Led => "M0,5 H5 M5,0 L11,5 L5,10 Z M11,0 V10 M11,5 H16",
                Kind::Inductor => "M0,5 H2 C2,-1 6,-1 6,5 C6,-1 10,-1 10,5 C10,-1 14,-1 14,5 H16",
                Kind::Crystal => "M0,5 H5 M5,0 V10 M7,1 H9 V9 H7 Z M11,0 V10 M11,5 H16",
                _ => "M0,5 H4 M4,5 L12,0 M12,5 H16",
            };
            out.push_str(&format!("<path transform=\"translate({},{}) scale({},{})\" d=\"{symbol}\" fill=\"none\" stroke=\"#ef9c46\" stroke-width=\"{}\"/>", n(x as f64*G), n(y as f64*G), n(w as f64*G/16.), n(h as f64*G/10.), n(0.5)));
        }
        let mut pads: Vec<_> = p.footprint.pads.iter().collect();
        if two { pads.sort_by(|a, b| a.number.cmp(&b.number)); }
        let left = pads.len().div_ceil(2);
        for (i, pad) in pads.into_iter().enumerate() {
            let lhs = i < left;
            let cy = if two { y+2 } else if lhs { y+2*(i as i32+1) } else { y+h-2*(i-left+1) as i32 };
            let edge = [if lhs { x } else { x+w }, cy];
            let at = [edge[0]+if lhs { -2 } else { 2 }, cy];
            let net = b.nets.iter().position(|n| n.pins.iter().any(|pin| pin.part == part_index && pin.number == pad.number));
            let name = format!("{}.{}", part.reference, pad.number);
            pins.insert(at, net);
            pins.insert([edge[0]+if lhs { -1 } else { 1 }, cy], net);
            if lhs { pin_left.insert(at); }
            if let Some(ground) = net.and_then(|net| power_net(&b.nets[net].name)) {
                for dx in -1..=1 { blocked.insert([at[0]+dx, at[1]+if ground { 1 } else { -1 }]); }
            }
            out.push_str(&format!("<path class=\"pin\" data-pin=\"{}\" data-at=\"{}\" d=\"M{} L{}\" fill=\"none\" stroke=\"#ef9c46\" stroke-width=\"{}\"/>", escape(&name), xy(at), xy(edge), xy(at), n(0.4)));
            text(&mut out, [(edge[0] as f64 + if lhs { -1.5 } else { 0.5 })*G, (cy as f64-0.3)*G], 1.8, &pad.number);
            if !two {
                out.push_str(&format!("<text x=\"{}\" y=\"{}\" text-anchor=\"{}\" font-family=\"sans-serif\" font-size=\"{}\" fill=\"#e2e8f0\">{}</text>", n((edge[0] as f64 + if lhs { 0.5 } else { -0.5 })*G), n(cy as f64*G+0.6), if lhs { "start" } else { "end" }, n(1.8), escape(net.map_or("NC", |n| b.nets[n].name.as_str()))));
            }
            if let Some(net) = net {
                endpoints[net].push(at);
            } else {
                out.push_str(&format!("<path class=\"nc\" data-pin=\"{}\" d=\"M{},{} l{},{} m{},0 l{},{}\" fill=\"none\" stroke=\"#e2e8f0\" stroke-width=\"{}\"/>", escape(&name), n(at[0] as f64*G-0.5), n(at[1] as f64*G-0.5), n(1.), n(1.), n(-1.), n(1.), n(-1.), n(0.3)));
            }
        }
        out.push_str("</g>");
    }
    let mut occupied = Occupied::new();
    for (net, points) in endpoints.iter().enumerate() {
        let name = &b.nets[net].name;
        if let Some(ground) = power_net(name) {
            for &at in points {
                out.push_str(&format!("<g class=\"power\" data-net=\"{}\" data-at=\"{}\" transform=\"translate({})\">", escape(name), xy(at), xy(at)));
                let symbol = if ground { "M0,0 V2.54 M-2.54,2.54 H2.54 M-1.7,3.5 H1.7 M-0.85,4.5 H0.85" }
                    else { "M0,0 V-2.54 M-1.27,-1.27 L0,-2.54 L1.27,-1.27" };
                out.push_str(&format!("<path d=\"{symbol}\" fill=\"none\" stroke=\"#7dd3fc\" stroke-width=\"{}\"/>", n(0.35)));
                if !ground {
                    let lhs = pin_left.contains(&at);
                    out.push_str(&format!("<text x=\"{}\" y=\"{}\" text-anchor=\"{}\" font-family=\"sans-serif\" font-size=\"{}\" fill=\"#e2e8f0\">{}</text>", n(if lhs { -1.5 } else { 1.5 }), n(-2.54), if lhs { "end" } else { "start" }, n(1.8), escape(name)));
                }
                out.push_str("</g>");
            }
        } else {
            // shortcut: bound schematic search to one million grid cells, use sparse routing for larger schematics.
            let tree = (points.len() <= 8 && i64::from(size[0])*i64::from(size[1]) <= 1_000_000)
                .then(|| wire_tree(points, &blocked, &occupied, &pins, net, size)).flatten();
            if let Some(tree) = tree {
                wires(&mut out, &tree, &points.iter().copied().collect(), name, &mut occupied);
            } else {
                for &at in points {
                    out.push_str(&format!("<text class=\"label\" data-net=\"{}\" data-at=\"{}\" x=\"{}\" y=\"{}\" font-family=\"sans-serif\" font-size=\"{}\" fill=\"#7dd3fc\">{}</text>", escape(name), xy(at), n(at[0] as f64*G+0.6), n(at[1] as f64*G-1.), n(1.8), escape(name)));
                }
            }
        }
    }
    out.push_str("</svg>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_names_match_whole_names() {
        for name in ["gnd", "VSSA", "AGND", "GNDD"] { assert_eq!(power_net(name), Some(true)); }
        for name in ["3V3", "+5V", "VCC", "1V8", "+12V"] { assert_eq!(power_net(name), Some(false)); }
        for name in ["VREF", "A3V", "GNDX", "+V", "3V3X", "3.3V", ""] { assert_eq!(power_net(name), None); }
    }

    #[test]
    fn wire_tree_crosses_only_straight_foreign_segments() {
        let occupied: Occupied = (-2_i32..=2).map(|dy| ([5, 5+dy], (12, dy.abs() == 2))).collect();
        let pins = BTreeMap::from([([2, 5], Some(0)), ([8, 5], Some(0))]);
        let tree = wire_tree(&[[2, 5], [8, 5]], &BTreeSet::new(), &occupied, &pins, 0, [12, 12]).unwrap();
        assert!(tree.contains_key(&[5, 5]));
        let mut out = String::new();
        wires(&mut out, &tree, &pins.keys().copied().collect(), "N", &mut occupied.clone());
        assert!(!out.contains("junction"));
        assert_eq!(out.matches("class=\"wire\"").count(), 1);
    }
}

