use super::{export, gerber, pour, route, Layer, Panel, Pours, PreparedBoard};
use i_overlay::{core::{fill_rule::FillRule, overlay_rule::OverlayRule}, float::single::SingleFloatOverlay};

pub(super) fn summary(panel: &Panel, b: &PreparedBoard) -> String {
    let [w, h] = panel.size(&b.spec.board);
    format!("Panel: {} x {} boards, {} x {} mm, {}.", panel.rows, panel.cols,
        crate::jsfmt::number(w), crate::jsfmt::number(h), panel.separation)
}

fn offsets(panel: &Panel, b: &PreparedBoard) -> Vec<(i64, i64, route::Point)> {
    let [x0, y0, x1, y1] = b.spec.board.bounds();
    let [rx, ry] = panel.rail_widths();
    (0..panel.rows).flat_map(|row| (0..panel.cols).map(move |col| (row, col,
        [rx + col as f64 * (x1 - x0 + panel.spacing()) - x0,
            ry + row as f64 * (y1 - y0 + panel.spacing()) - y0]))).collect()
}

fn shifted(at: route::Point, offset: route::Point) -> route::Point {
    [at[0] + offset[0], at[1] + offset[1]]
}

fn rail_points(panel: &Panel, b: &PreparedBoard, tooling: bool) -> Vec<route::Point> {
    let [w, h] = panel.size(&b.spec.board);
    let [rx, ry] = panel.rail_widths();
    let moat = if panel.separation == "tabs" { panel.spacing() / 2. } else { 0. };
    let inset = if tooling { 1.5 } else { 5. };
    if ry > 0. {
        let edge = (ry - moat) / 2.;
        if tooling { vec![[inset, edge], [w - inset, edge], [inset, h - edge]] }
        else { vec![[inset, edge], [w - inset, edge], [0.4 * w, h - edge]] }
    } else {
        let edge = (rx - moat) / 2.;
        if tooling { vec![[edge, inset], [edge, h - inset], [w - edge, inset]] }
        else { vec![[edge, inset], [edge, h - inset], [w - edge, 0.4 * h]] }
    }
}

fn edges(panel: &Panel, b: &PreparedBoard) -> (Vec<pour::Contour>, Vec<route::Point>) {
    let [w, h] = panel.size(&b.spec.board);
    let outer = pour::rect([w / 2., h / 2.], [w, h]);
    if panel.separation == "vscore" { return (vec![outer], vec![]); }
    let [x0, y0, x1, y1] = b.spec.board.bounds();
    let [bw, bh] = [x1 - x0, y1 - y0];
    let [rx, ry] = panel.rail_widths();
    let s = panel.spacing();
    let mut boards = vec![];
    let mut gaps = vec![];
    let mut bridges = vec![];
    let mut bites = vec![];
    for (row, col, offset) in offsets(panel, b) {
        let [left, bottom] = shifted([x0, y0], offset);
        let at = [left + bw / 2., bottom + bh / 2.];
        boards.push(pour::rect(at, [bw, bh]));
        gaps.push(pour::rect(at, [bw + s, bh + s]));
        for (vertical, start, length, faces, direction) in [
            (true, [left, bottom], bh, col > 0 || rx > 0., -1.),
            (true, [left + bw, bottom], bh, col + 1 < panel.cols || rx > 0., 1.),
            (false, [left, bottom], bw, row > 0 || ry > 0., -1.),
            (false, [left, bottom + bh], bw, row + 1 < panel.rows || ry > 0., 1.),
        ] {
            if !faces { continue; }
            for tab in 0..panel.tabs {
                let along = length * (tab + 1) as f64 / (panel.tabs + 1) as f64;
                let edge = if vertical { [start[0], start[1] + along] } else { [start[0] + along, start[1]] };
                let centre = if vertical { [edge[0] + direction * s / 4., edge[1]] }
                    else { [edge[0], edge[1] + direction * s / 4.] };
                let size = if vertical { [s / 2. + 0.002, panel.tab_width] }
                    else { [panel.tab_width, s / 2. + 0.002] };
                bridges.push(pour::rect(centre, size));
                if panel.mousebites {
                    let count = (panel.tab_width / 0.8).floor() as usize + 1;
                    for i in 0..count {
                        let delta = (i as f64 - (count - 1) as f64 / 2.) * 0.8;
                        bites.push(if vertical { [edge[0], edge[1] + delta] } else { [edge[0] + delta, edge[1]] });
                    }
                }
            }
        }
    }
    let gap = gaps.overlay(&boards, OverlayRule::Difference, FillRule::NonZero);
    let cut = gap.overlay(&bridges, OverlayRule::Difference, FillRule::NonZero);
    let cut = cut.overlay(&outer, OverlayRule::Intersect, FillRule::NonZero);
    let mut contours = vec![outer];
    contours.extend(cut.into_iter().flatten());
    (contours, bites)
}

pub(super) fn files(prepared: &PreparedBoard, board: &route::Board, routes: &route::Routes,
    pours: &Pours, name: &str) -> Vec<(String, String)>
{
    let panel = prepared.spec.panel.as_ref().unwrap();
    let mut b = board.clone();
    b.pads.clear(); b.holes.clear(); b.fixed.clear();
    let mut r = routes.clone();
    r.tracks.clear(); r.vias.clear(); r.teardrops.clear();
    let mut p = prepared.clone();
    p.parts.clear(); p.spec.parts.clear();
    let mut panel_pours = vec![];
    for (row, col, offset) in offsets(panel, prepared) {
        b.pads.extend(board.pads.iter().map(|pad| {
            let mut pad = pad.clone(); pad.at = shifted(pad.at, offset); pad
        }));
        b.holes.extend(board.holes.iter().map(|&(at, d)| (shifted(at, offset), d)));
        b.fixed.extend(board.fixed.iter().map(|trace| {
            let mut trace = trace.clone(); trace.track.a = shifted(trace.track.a, offset);
            trace.track.b = shifted(trace.track.b, offset); trace
        }));
        r.tracks.extend(routes.tracks.iter().map(|track| {
            let mut track = track.clone(); track.a = shifted(track.a, offset);
            track.b = shifted(track.b, offset); track
        }));
        r.vias.extend(routes.vias.iter().map(|via| {
            let mut via = via.clone(); via.at = shifted(via.at, offset); via
        }));
        r.teardrops.extend(routes.teardrops.iter().map(|drop| {
            let mut drop = drop.clone(); drop.points.iter_mut().for_each(|at| *at = shifted(*at, offset)); drop
        }));
        for part in &prepared.parts {
            let mut spec = prepared.spec.parts[part.part].clone();
            spec.reference = format!("{}_r{}c{}", spec.reference, row + 1, col + 1);
            let mut part = part.clone(); part.part = p.spec.parts.len(); part.at = shifted(part.at, offset);
            p.parts.push(part); p.spec.parts.push(spec);
        }
        panel_pours.extend(pours.iter().map(|(net, layer, shapes)| (*net, *layer,
            shapes.iter().map(|shape| shape.iter().map(|ring|
                ring.iter().map(|at| shifted(*at, offset)).collect()).collect()).collect())));
    }
    let (contours, bites) = edges(panel, prepared);
    b.outline = contours[0].clone(); b.size = panel.size(&prepared.spec.board); b.origin = [0.; 2];
    b.holes.extend(bites.into_iter().map(|at| (at, 0.5)));
    if panel.tooling { b.holes.extend(rail_points(panel, prepared, true).into_iter().map(|at| (at, 2.))); }
    let name = format!("{name}-panel");
    let mut files = vec![(format!("fab/{name}/{name}-B_Cu.gbr"),
        gerber::copper(&b, &r, &panel_pours, Layer::Bottom))];
    files.extend(gerber::files(&p, &b, &r, &panel_pours, &name));
    let fiducials = if panel.fiducials { rail_points(panel, prepared, false) } else { vec![] };
    for (path, text) in &mut files {
        if path.ends_with("Edge_Cuts.gbr") { *text = gerber::contours("Profile,NP", &contours, true); }
        if !fiducials.is_empty() {
            if path.ends_with("F_Cu.gbr") || path.ends_with("B_Cu.gbr") { gerber::fiducials(text, &fiducials, 1.); }
            if path.ends_with("F_Mask.gbr") || path.ends_with("B_Mask.gbr") { gerber::fiducials(text, &fiducials, 2.); }
        }
    }
    if panel.separation == "vscore" {
        let [rx, ry] = panel.rail_widths();
        let [bw, bh] = board.size;
        let [w, h] = b.size;
        let mut scores = vec![];
        for col in 0..=panel.cols {
            if (col > 0 && col < panel.cols) || rx > 0. {
                let x = rx + col as f64 * bw; scores.push(vec![[x, 0.], [x, h]]);
            }
        }
        for row in 0..=panel.rows {
            if (row > 0 && row < panel.rows) || ry > 0. {
                let y = ry + row as f64 * bh; scores.push(vec![[0., y], [w, y]]);
            }
        }
        files.push((format!("fab/{name}/{name}-VScore.gbr"), gerber::contours("Other,VScore", &scores, false)));
    }
    files.push((format!("fab/{name}/{name}-pnp.csv"), export::pnp(&p)));
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_cutouts_close_and_leave_bridges_to_neighbours_and_rails() {
        let b = super::super::prepare(r#"{"board":{"width":20,"height":15},"panel":{"rows":2,"cols":3}}"#).unwrap();
        let (contours, bites) = edges(b.spec.panel.as_ref().unwrap(), &b);
        assert_eq!(contours[0], pour::rect([32., 21.], [64., 42.]));
        assert_eq!(bites.len(), 160);
        let cut = |at| contours[1..].iter().filter(|ring| super::super::place::inside(at, ring)).count() % 2 == 1;
        assert!(cut([10., 21.])); // inter-board gap away from a tab
        assert!(!cut([20. / 3., 21.])); // opposing tabs bridge the entire gap
        assert!(cut([10., 4.5])); // rail routed away from board
        assert!(!cut([20. / 3., 4.5])); // tab joins bottom rail
        assert!(!cut([10., 10.])); // board material
        assert!(!cut([10., 2.])); // rail material
        assert!(contours[1..].iter().all(|ring| ring.len() >= 4 && pour::signed_area(ring).abs() > 0.));
    }

    #[test]
    fn four_layer_panels_step_pours_fixed_copper_vias_and_spanned_drills() {
        let p = super::super::prepare(r#"{"board":{"outline":[[2,3],[22,3],[22,18],[2,18]],"layers":4},"rules":{"blind_vias":true},"panel":{"rows":2,"cols":2,"rail_sides":"both"},"nets":{"N":[]}}"#).unwrap();
        let mut b = route::Board::from_prepared(&p).unwrap();
        b.fixed.push(route::FixedTrace { track: route::Track { a: [5., 7.], b: [6., 7.], layer: 0, width: 0.2, net: 0 }, nets: vec![0], part: 0 });
        let mut r = route::route(&b);
        r.vias.extend([route::Via { at: [8., 7.], net: 0, span: (0, 1) },
            route::Via { at: [9., 7.], net: 0, span: (1, 2) }]);
        r.teardrops.push(route::Teardrop { points: vec![[5., 7.], [6., 7.], [6., 8.]], net: 0, layer: 0 });
        let pours = vec![(0, Layer::Inner1, vec![vec![pour::rect([12., 10.], [5.; 2]), pour::rect([12., 10.], [1.; 2])]])];
        let files = files(&p, &b, &r, &pours, "test");
        let file = |suffix: &str| &files.iter().find(|(path, _)| path.ends_with(suffix)).unwrap().1;
        assert_eq!(file("PTH-1-2.drl").lines().filter(|l| l.starts_with('X')).count(), 4);
        assert_eq!(file("PTH-2-3.drl").lines().filter(|l| l.starts_with('X')).count(), 4);
        assert_eq!(file("In1_Cu.gbr").matches("G36*").count(), 8);
        assert_eq!(file("In1_Cu.gbr").matches("%LPC*%").count(), 4);
        assert_eq!(file("F_Cu.gbr").matches("G36*").count(), 4);
        assert!(file("F_Cu.gbr").contains("X8000000Y9000000D02*\nX9000000Y9000000D01*\n"));
        assert!(file("F_Cu.gbr").contains("X30000000Y26000000D02*\nX31000000Y26000000D01*\n"));
        assert_eq!(file("F_Cu.gbr").matches("D03*").count(), 7);
        assert_eq!(file("B_Cu.gbr").matches("D03*").count(), 3);
        assert_eq!(file("F_Paste.gbr").matches("D03*").count(), 0);
    }
}
