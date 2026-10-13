use super::{gerber, pour, route, Kind, Layer, Pours, PreparedBoard};
use crate::jsfmt::fixed_string;

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

pub(super) fn schematic(b: &PreparedBoard) -> String {
    // shortcut: grid of symbols with net labels instead of drawn wires, upgrade to wire routing when users ask for readable schematics.
    let rows = b
        .parts
        .iter()
        .map(|p| p.footprint.pads.len())
        .max()
        .unwrap_or(0);
    let cell_height = 26. + rows as f64 * 4.;
    let label_length = b
        .nets
        .iter()
        .map(|net| net.name.chars().count())
        .max()
        .unwrap_or(0);
    let cell_width = 60_f64.max(30. + label_length as f64 * 2.);
    let columns = b.parts.len().clamp(1, 4);
    let height = cell_height * b.parts.len().div_ceil(columns).max(1) as f64;
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"#101923\"/>", n(cell_width * columns as f64), n(height));
    for (i, p) in b.parts.iter().enumerate() {
        let part = &b.spec.parts[p.part];
        let x = (i % columns) as f64 * cell_width + 4.;
        let y = (i / columns) as f64 * cell_height + 5.;
        text(
            &mut out,
            [x, y],
            3.,
            &format!("{} {}", part.reference, part.value.as_deref().unwrap_or("")),
        );
        let symbol = match part.kind {
            Kind::Resistor => "M0,5 H3 L4,2 L6,8 L8,2 L10,8 L12,2 L13,5 H16",
            Kind::Capacitor => "M0,5 H7 M7,0 V10 M9,0 V10 M9,5 H16",
            Kind::Diode | Kind::Led => "M0,5 H5 M5,0 L11,5 L5,10 Z M11,0 V10 M11,5 H16",
            Kind::Inductor => "M0,5 H2 C2,-1 6,-1 6,5 C6,-1 10,-1 10,5 C10,-1 14,-1 14,5 H16",
            Kind::Antenna => "M8,10 V0 M2,0 L8,6 L14,0",
            Kind::Button | Kind::Switch => "M0,5 H4 M4,5 L12,0 M12,5 H16",
            _ => "M2,0 H14 V10 H2 Z M0,3 H2 M0,7 H2 M14,3 H16 M14,7 H16",
        };
        out.push_str(&format!("<path transform=\"translate({}, {})\" d=\"{symbol}\" fill=\"none\" stroke=\"#ef9c46\" stroke-width=\"0.5\"/>", n(x), n(y + 3.)));
        for (j, pad) in p.footprint.pads.iter().enumerate() {
            let net = b
                .nets
                .iter()
                .find(|net| {
                    net.pins
                        .iter()
                        .any(|pin| pin.part == p.part && pin.number == pad.number)
                })
                .map_or("NC", |net| net.name.as_str());
            let alias = part.pins.get(&pad.number).map_or("", String::as_str);
            text(
                &mut out,
                [x, y + 18. + j as f64 * 4.],
                2.5,
                &format!("{} {}: {}", pad.number, alias, net),
            );
        }
    }
    out.push_str("</svg>");
    out
}
