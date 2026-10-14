use super::{gerber, panel, pour, route, svg, PreparedBoard};
use crate::jsfmt::fixed_string;
use std::collections::BTreeMap;

fn n(v: f64) -> String { fixed_string(v, 6) }

fn dimension(v: f64) -> String {
    let mut value = fixed_string(v, 2);
    if !value.contains('.') { value.push('.'); }
    while value.len() - value.find('.').unwrap() - 1 < 2 { value.push('0'); }
    value
}

fn text(out: &mut String, at: route::Point, size: f64, value: &str) {
    out.push_str(&format!("<text x=\"{}\" y=\"{}\" font-size=\"{}\">{}</text>\n", n(at[0]), n(at[1]), n(size), svg::escape(value)));
}

fn line(out: &mut String, a: route::Point, b: route::Point) {
    out.push_str(&format!("<path d=\"M{},{} L{},{}\"/>\n", n(a[0]), n(a[1]), n(b[0]), n(b[1])));
}

fn symbol(out: &mut String, at: route::Point, radius: f64, tool: usize) {
    let [x, y] = at;
    let points = match tool % 5 {
        0 => {
            out.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>\n", n(x), n(y), n(radius)));
            return;
        }
        1 => pour::rect(at, [2. * radius; 2]),
        2 => vec![[x, y - radius], [x + radius, y + radius], [x - radius, y + radius]],
        3 => {
            line(out, [x - radius, y - radius], [x + radius, y + radius]);
            line(out, [x - radius, y + radius], [x + radius, y - radius]);
            return;
        }
        _ => vec![[x, y - radius], [x + radius, y], [x, y + radius], [x - radius, y]],
    };
    out.push_str(&format!("<path d=\"{}\"/>\n", svg::path(&[points])));
}

pub(super) fn drawing(prepared: &PreparedBoard, b: &route::Board, r: &route::Routes,
    name: &str, impedance: Option<&str>) -> String
{
    // The same holes and tool rounding drive the drawing and Excellon writers.
    let mut tools = BTreeMap::<(i64, bool, bool, usize, usize), Vec<gerber::DrillHole>>::new();
    for plated in [true, false] {
        for hole in gerber::drills(b, r, plated) {
            tools.entry(((hole.tool * 1e6).round() as i64, plated, hole.slot.is_some(), 1, b.layers))
                .or_default().push(hole);
        }
    }
    for via in r.vias.iter().filter(|v| v.span != (0, b.layers - 1)) {
        tools.entry(((b.via[1] * 1e6).round() as i64, true, false, via.span.0 + 1, via.span.1 + 1))
            .or_default().push(gerber::DrillHole { at: via.at, tool: b.via[1], slot: None });
    }
    let board = &prepared.spec.board;
    let rules = &prepared.spec.rules;
    let mut stack = vec![("F.Cu", 0.035, false)];
    if b.layers == 4 {
        stack.extend([("Prepreg", board.prepreg, true), ("In1.Cu", 0.035, false),
            ("Core", board.thickness - 2. * board.prepreg - 4. * 0.035, true),
            ("In2.Cu", 0.035, false), ("Prepreg", board.prepreg, true)]);
    } else { stack.push(("Core", board.thickness - 0.07, true)); }
    stack.push(("B.Cu", 0.035, false));
    let mut notes = vec![format!("Board thickness: {} mm. Dielectric er: {}.", n(board.thickness), n(board.er)),
        format!("Min trace: {} mm; min clearance: {} mm; via size/drill: {}/{} mm.",
            n(rules.trace), n(rules.clearance), n(b.via[0]), n(b.via[1]))];
    if let Some(impedance) = impedance { notes.push(impedance.into()); }
    if rules.via_in_pad { notes.push("Filled/capped vias-in-pad are required.".into()); }
    if r.vias.iter().any(|v| v.span != (0, b.layers - 1)) {
        notes.push("Blind/buried vias: drill spans use physical copper layer pairs in the drill table.".into());
    }
    if let Some(panel) = &prepared.spec.panel {
        notes.push(panel::summary(panel, prepared));
        if panel.separation == "vscore" {
            notes.push(format!("V-score: full-panel score lines in {name}-panel-VScore.gbr, including rail boundaries."));
        } else {
            notes.push(format!("Panel tabs: {} per facing edge, {} mm wide; mouse-bites: {}.",
                panel.tabs, n(panel.tab_width), panel.mousebites));
        }
    }
    let mut wrapped = vec![];
    for note in notes {
        let mut row = String::new();
        for word in note.split_whitespace() {
            if !row.is_empty() && row.chars().count() + word.chars().count() > 125 {
                wrapped.push(std::mem::take(&mut row));
            }
            if !row.is_empty() { row.push(' '); }
            row.push_str(word);
        }
        wrapped.push(row);
    }
    let stack_y = 36. + tools.len() as f64 * 5.;
    let notes_y = (stack_y + stack.len() as f64 * 5. + 18.).max(143.);
    let height = (notes_y + wrapped.len() as f64 * 5. + 10.).max(210.);
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"297mm\" height=\"210mm\" viewBox=\"0 0 297 {}\">\n<rect width=\"297\" height=\"{}\" fill=\"white\"/>\n<g font-family=\"sans-serif\" fill=\"#171717\">\n", n(height), n(height));
    text(&mut out, [10., 12.], 5., &format!("{name} — fabrication drawing"));
    text(&mut out, [10., 19.], 3., "Dimensions in mm; board-centred origin, +y up.");
    let [x0, y0, x1, y1] = board.bounds();
    let scale = (125. / (x1.max(0.) - x0.min(0.))).min(95. / (y1.max(0.) - y0.min(0.)));
    let screen = |[x, y]: route::Point| [20. + (x - x0.min(0.)) * scale, 32. + (y1.max(0.) - y) * scale];
    out.push_str("<g fill=\"none\" stroke=\"#171717\" stroke-width=\"0.2\">\n");
    let outline: Vec<_> = b.outline.iter().map(|p| screen(*p)).collect();
    out.push_str(&format!("<path class=\"board-outline\" d=\"{}\"/>\n", svg::path(&[outline])));
    for (tool, (_, holes)) in tools.iter().enumerate() {
        for hole in holes {
            if let Some([p1, p2]) = hole.slot {
                let a = screen(p1); let z = screen(p2);
                let length = route::distance(a, z);
                let radius = hole.tool * scale / 2.;
                let angle = (z[1] - a[1]).atan2(z[0] - a[0]).to_degrees();
                out.push_str(&format!("<rect class=\"slot\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" transform=\"translate({},{}) rotate({})\"/>\n",
                    n(-radius), n(-radius), n(length + 2. * radius), n(2. * radius), n(radius), n(a[0]), n(a[1]), n(angle)));
            } else { symbol(&mut out, screen(hole.at), (hole.tool * scale / 2.).max(0.45), tool); }
        }
    }
    let origin = screen([0.; 2]);
    line(&mut out, [origin[0] - 2., origin[1]], [origin[0] + 2., origin[1]]);
    line(&mut out, [origin[0], origin[1] - 2.], [origin[0], origin[1] + 2.]);
    let [left, top] = screen([x0, y1]);
    let [right, bottom] = screen([x1, y0]);
    line(&mut out, [left, bottom + 6.], [right, bottom + 6.]);
    line(&mut out, [left - 6., top], [left - 6., bottom]);
    for x in [left, right] { line(&mut out, [x, bottom + 2.], [x, bottom + 8.]); }
    for y in [top, bottom] { line(&mut out, [left - 8., y], [left - 2., y]); }
    out.push_str("</g>\n");
    text(&mut out, [origin[0] + 2., origin[1] - 2.], 2.8, "0,0");
    text(&mut out, [(left + right) / 2. - 8., bottom + 11.], 3., &format!("{} mm", dimension(x1 - x0)));
    out.push_str(&format!("<text transform=\"translate({},{}) rotate(-90)\" font-size=\"3\">{} mm</text>\n",
        n(left - 9.), n((top + bottom) / 2. + 8.), dimension(y1 - y0)));
    text(&mut out, [168., 26.], 3.5, "Drill table");
    for (x, label) in [(168., "Tool"), (179., "Symbol"), (191., "Diameter"), (219., "Type / layers"), (280., "Count")] {
        text(&mut out, [x, 32.], 2.8, label);
    }
    let mut total = 0;
    for (tool, (&(diameter, plated, slot, first, last), holes)) in tools.iter().enumerate() {
        let y = 38. + tool as f64 * 5.;
        let kind = if plated { "Plated" } else { "NPTH" };
        let kind = format!("{kind}{} L{first}-L{last}", if slot { " slot" } else { "" });
        out.push_str(&format!("<g class=\"drill-row\" data-diameter=\"{}\" data-count=\"{}\" data-span=\"{first}-{last}\">\n", n(diameter as f64 / 1e6), holes.len()));
        text(&mut out, [168., y], 2.8, &format!("T{}", tool + 1));
        out.push_str("<g fill=\"none\" stroke=\"#171717\" stroke-width=\"0.2\">\n");
        symbol(&mut out, [181., y - 1.], 1., tool);
        out.push_str("</g>\n");
        text(&mut out, [191., y], 2.8, &format!("{} mm", n(diameter as f64 / 1e6)));
        text(&mut out, [219., y], 2.8, &kind);
        text(&mut out, [280., y], 2.8, &holes.len().to_string());
        out.push_str("</g>\n");
        total += holes.len();
    }
    text(&mut out, [168., 38. + tools.len() as f64 * 5.], 3., &format!("Total: {total} drilled holes / slots"));
    text(&mut out, [168., stack_y + 12.], 3.5, "Stackup (physical order)");
    for (i, (layer, thickness, dielectric)) in stack.iter().enumerate() {
        let y = stack_y + 18. + i as f64 * 5.;
        out.push_str(&format!("<g class=\"stackup-row\" data-thickness=\"{}\">\n", n(*thickness)));
        text(&mut out, [168., y], 2.8, layer);
        text(&mut out, [206., y], 2.8, &format!("{} mm", n(*thickness)));
        if *dielectric { text(&mut out, [249., y], 2.8, &format!("er {}", n(board.er))); }
        out.push_str("</g>\n");
    }
    text(&mut out, [168., stack_y + 18. + stack.len() as f64 * 5.], 3., &format!("Total thickness: {} mm", n(board.thickness)));
    text(&mut out, [10., notes_y + 7.], 3.5, "Fabrication notes");
    for (i, note) in wrapped.iter().enumerate() { text(&mut out, [10., notes_y + 13. + i as f64 * 5.], 3., note); }
    out.push_str("</g>\n</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_counts_match_excellon_and_stackup_sums_for_two_and_four_layers() {
        for layers in [2, 4] {
            let prepared = super::super::prepare(&format!(r#"{{"board":{{"width":30,"height":20,"layers":{layers}}},"rules":{{"via_in_pad":true}},"parts":[{{"ref":"J1","kind":"header","footprint":"usb_c_16","at":[0,0]}}],"holes":[{{"at":[10,5],"d":3.2}}],"nets":{{"N":[]}}}}"#)).unwrap();
            let b = route::Board::from_prepared(&prepared).unwrap();
            let mut r = route::route(&b);
            if layers == 4 {
                r.vias.extend([route::Via { at: [-10., 0.], net: 0, span: (0, 1) },
                    route::Via { at: [10., 0.], net: 0, span: (1, 2) }]);
            }
            let drawing = drawing(&prepared, &b, &r, "test", Some("Impedance: <clock> 50 ohm -> 0.344 mm"));
            let files = gerber::files(&prepared, &b, &r, &vec![], "test");
            let drill_count: usize = files.iter().filter(|(path, _)| path.ends_with(".drl"))
                .map(|(_, text)| text.lines().filter(|l| l.starts_with('X')).count()).sum();
            let table_count: usize = drawing.split("data-count=\"").skip(1)
                .map(|s| s.split('"').next().unwrap().parse::<usize>().unwrap()).sum();
            assert_eq!(table_count, drill_count);
            assert_eq!(drawing.matches("class=\"slot\"").count(), 4);
            let total_thickness: f64 = drawing.split("data-thickness=\"").skip(1)
                .map(|s| s.split('"').next().unwrap().parse::<f64>().unwrap()).sum();
            assert!((total_thickness - prepared.spec.board.thickness).abs() < 0.001);
            assert!(drawing.contains("30.00 mm") && drawing.contains("20.00 mm"));
            assert!(drawing.contains("Filled/capped vias-in-pad are required."));
            assert!(drawing.contains("Impedance: &lt;clock&gt; 50 ohm -&gt; 0.344 mm"));
            if layers == 4 {
                assert!(drawing.contains("data-span=\"1-2\"") && drawing.contains("data-span=\"2-3\""));
                assert!(drawing.contains("Blind/buried vias:"));
            }
            assert_eq!(drawing, super::drawing(&prepared, &b, &r, "test", Some("Impedance: <clock> 50 ohm -> 0.344 mm")));
        }
    }
}
