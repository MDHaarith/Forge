use super::{drc::Shape, place, pour, route::{self, Board, Point, Routes, Teardrop, Track}};

pub(super) fn length(tracks: &[Track], net: usize) -> f64 {
    // shortcut: via barrels and in-pad length excluded, include them when propagation-delay matching is supported.
    tracks.iter().filter(|t| t.net == net).map(|t| route::distance(t.a, t.b)).sum()
}

pub(super) fn coupled(tracks: &[Track], p: usize, n: usize, spacing: f64) -> (f64, f64) {
    let mut total = 0.;
    let mut gaps = vec![];
    for a in tracks.iter().filter(|t| t.net == p) {
        let len = route::distance(a.a, a.b);
        if len < 1e-9 { continue; }
        let u = [(a.b[0] - a.a[0]) / len, (a.b[1] - a.a[1]) / len];
        let projection = |q: Point| (q[0] - a.a[0]) * u[0] + (q[1] - a.a[1]) * u[1];
        let perpendicular = |q: Point| (q[0] - a.a[0]) * u[1] - (q[1] - a.a[1]) * u[0];
        for b in tracks.iter().filter(|t| t.net == n && t.layer == a.layer) {
            let other = route::distance(b.a, b.b);
            if other < 1e-9 { continue; }
            let dot = ((b.b[0] - b.a[0]) * u[0] + (b.b[1] - b.a[1]) * u[1]) / other;
            if dot.abs() < 1_f64.to_radians().cos() { continue; }
            let distance = ((perpendicular(b.a) + perpendicular(b.b)) / 2.).abs();
            if (distance - spacing).abs() > 0.02 + 1e-6 { continue; }
            let (x, y) = (projection(b.a), projection(b.b));
            let overlap = len.min(x.max(y)) - 0_f64.max(x.min(y));
            if overlap > 1e-9 {
                total += overlap;
                gaps.push(distance - (a.width + b.width) / 2.);
            }
        }
    }
    gaps.sort_by(f64::total_cmp);
    let median = if gaps.is_empty() { 0. } else { (gaps[(gaps.len() - 1) / 2] + gaps[gaps.len() / 2]) / 2. };
    (total, median)
}

pub(super) fn pad_shape(p: &route::Pad, drill: bool) -> Shape {
    let size = if drill { p.drill.unwrap() } else { p.size };
    if p.round || drill {
        let axis = if size[0] > size[1] { 0 } else { 1 };
        let mut end = [0.; 2];
        end[axis] = (size[0] - size[1]).abs() / 2.;
        Shape::Stroke(p.point(end), p.point(end.map(|v| -v)), size[0].min(size[1]) / 2.)
    } else {
        Shape::Region(vec![vec![pour::rect([0.; 2], size).into_iter().map(|q| p.point(q)).collect()]])
    }
}

/// Continuous geometry shared with saved-copper DRC; no grid approximation in tuning or teardrops.
pub(super) fn clear_shape(b: &Board, r: &Routes, net: usize, layer: usize, shape: &Shape) -> bool {
    let eps = 1e-7;
    if shape.edge_gap(&b.outline) + eps < b.edge { return false; }
    for p in &b.pads {
        if p.plated && (p.net == net || p.layers & (1 << layer) == 0) { continue; }
        if !p.plated && p.drill.is_none() { continue; }
        if shape.gap(&pad_shape(p, !p.plated)) + eps < b.clearance(net).max(b.clearance(p.net)) { return false; }
    }
    for t in r.tracks.iter().chain(b.fixed.iter().filter(|f| !f.nets.contains(&net)).map(|f| &f.track)) {
        if t.net != net && t.layer == layer && shape.gap(&Shape::Stroke(t.a, t.b, t.width / 2.)) + eps < b.clearance(net).max(b.clearance(t.net)) { return false; }
    }
    for v in r.vias.iter().chain(&b.fixed_vias) {
        if v.net != net && v.on(layer, b.layers) && shape.gap(&Shape::Disk(v.at, b.via[0] / 2.)) + eps < b.clearance(net).max(b.clearance(v.net)) { return false; }
    }
    for t in &r.teardrops {
        if t.net != net && t.layer == layer && shape.gap(&Shape::Region(vec![vec![t.points.clone()]])) + eps < b.clearance(net).max(b.clearance(t.net)) { return false; }
    }
    for &(at, d) in &b.holes {
        if shape.gap(&Shape::Disk(at, d / 2.)) + eps < b.clearance(net) { return false; }
    }
    for &(at, size, layers) in &b.keepouts {
        // Regions intersecting a keepout have zero gap; touching its boundary is also excluded.
        if layers & (1 << layer) != 0 && shape.gap(&Shape::Region(vec![vec![pour::rect(at, size)]])) <= eps { return false; }
    }
    b.antenna_keepouts.iter().all(|p| shape.gap(&Shape::Region(vec![vec![p.clone()]])) > eps)
}

pub(super) fn clear_track(b: &Board, r: &Routes, t: &Track) -> bool {
    !b.planes.iter().any(|p| p.1 == t.layer)
        && clear_shape(b, r, t.net, t.layer, &Shape::Stroke(t.a, t.b, t.width / 2.))
}

pub(super) fn clear_via(b: &Board, r: &Routes, v: &route::Via) -> bool {
    let shape = Shape::Disk(v.at, b.via[0] / 2.);
    for l in 0..b.layers {
        if !v.on(l, b.layers) { continue; }
        if !clear_shape(b, r, v.net, l, &shape) { return false; }
        if b.pads.iter().any(|p| p.layers & (1 << l) != 0 && p.drill.is_none()
            && !(b.via_in_pad && p.net == v.net) && shape.gap(&pad_shape(p, false)) <= 1e-7) { return false; }
    }
    for p in b.pads.iter().filter(|p| p.drill.is_some()) {
        if pad_shape(p, true).gap(&Shape::Disk(v.at, b.via[1] / 2.)) + 1e-7 < b.clearance(v.net).max(b.clearance(p.net)) { return false; }
    }
    r.vias.iter().chain(&b.fixed_vias).all(|u| route::distance(v.at, u.at) + 1e-7 >= b.via[1] + b.clearance(v.net).max(b.clearance(u.net)))
}

pub(super) fn tune_net(b: &Board, r: &mut Routes, net: usize, target: f64, tolerance: f64, small: Option<f64>) {
    let mut segments: Vec<_> = r.tracks.iter().filter(|t| t.net == net).cloned().collect();
    segments.sort_by(|a, b| route::distance(b.a, b.b).total_cmp(&route::distance(a.a, a.b)));
    let mut bends = 0;
    for original in segments {
        let len = route::distance(original.a, original.b);
        let spacing = original.width + b.clearance(net).max(2. * original.width);
        if len < 3. * spacing { continue; }
        let unit = [(original.b[0] - original.a[0]) / len, (original.b[1] - original.a[1]) / len];
        let at = |x: f64, y: f64| [original.a[0] + unit[0] * x - unit[1] * y, original.a[1] + unit[1] * x + unit[0] * y];
        let mut points = vec![original.a];
        let mut added = 0.;
        let mut x = spacing;
        while x + 2. * spacing <= len {
            let debt = target - length(&r.tracks, net) - added;
            if debt <= tolerance / 2. + 1e-7 { break; }
            // shortcut: 4096 tuning positions per net, use delay-based optimization when larger buses need more.
            if bends >= 4096 { break; }
            bends += 1;
            let mut chosen = None;
            for step in (4..=10).rev() {
                let max = small.unwrap_or(5. * original.width);
                let amplitude = (step as f64 * 0.5 * original.width).min(max).min(debt / 2.);
                for side in [1., -1.] {
                    let bump = [at(x, 0.), at(x, side * amplitude), at(x + spacing, side * amplitude), at(x + spacing, 0.)];
                    if bump.windows(2).all(|p| {
                        let candidate = Track { a: p[0], b: p[1], ..original.clone() };
                        clear_track(b, r, &candidate) && r.tracks.iter().filter(|t| t.net == net && t.layer == original.layer && **t != original).all(|t| {
                            route::segment_segment(candidate.a, candidate.b, t.a, t.b) + 1e-7 >= (candidate.width + t.width) / 2. + b.clearance(net)
                        })
                    }) {
                        chosen = Some((bump, amplitude)); break;
                    }
                }
                if chosen.is_some() { break; }
            }
            if let Some((bump, amplitude)) = chosen {
                points.extend(bump);
                added += 2. * amplitude;
            }
            x += 2. * spacing;
        }
        if added > 1e-9 {
            points.push(original.b);
            let index = r.tracks.iter().position(|t| *t == original).unwrap();
            let tracks = points.windows(2).filter(|p| route::distance(p[0], p[1]) > 1e-9)
                .map(|p| Track { a: p[0], b: p[1], ..original.clone() });
            r.tracks.splice(index..=index, tracks);
        }
        if target - length(&r.tracks, net) <= tolerance / 2. + 1e-7 { break; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coupled_length_uses_parallel_overlap_and_layers() {
        let tracks = vec![
            Track { a: [0.,0.], b: [10.,0.], width: 0.2, layer: 0, net: 0 },
            Track { a: [8.,0.35], b: [2.,0.35], width: 0.2, layer: 0, net: 1 },
            Track { a: [0.,0.35], b: [10.,0.35], width: 0.2, layer: 1, net: 1 },
            Track { a: [10.,0.35], b: [10.,5.], width: 0.2, layer: 0, net: 1 }];
        let (length, gap) = coupled(&tracks, 0, 1, 0.35);
        assert_eq!(length, 6.);
        assert!((gap - 0.15).abs() < 1e-9);
        assert_eq!(super::length(&tracks, 0), 10.);
    }
}

pub(super) fn finish(b: &Board, r: &mut Routes) {
    for pair in &b.pairs {
        let (p, n) = (length(&r.tracks, pair.p), length(&r.tracks, pair.n));
        // shortcut: single-line skew bumps, upgrade to coupled meanders later.
        tune_net(b, r, if p < n { pair.p } else { pair.n }, p.max(n), pair.skew, Some(pair.gap + pair.width));
    }
    for (_, nets, tolerance) in &b.length_groups {
        let target = nets.iter().map(|&n| length(&r.tracks, n)).fold(0., f64::max);
        for &net in nets { tune_net(b, r, net, target, *tolerance, None); }
    }
    if b.teardrops { teardrops(b, r); }
}

fn teardrops(b: &Board, r: &mut Routes) {
    for t in r.tracks.clone() {
        for (end, other) in [(t.a, t.b), (t.b, t.a)] {
            let mut anchors = vec![];
            for p in b.pads.iter().filter(|p| p.net == t.net && p.layers & (1 << t.layer) != 0) {
                if !pad_shape(p, false).contains(end) { continue; }
                let dim = p.size[0].min(p.size[1]);
                let d = route::distance(p.at, other);
                if d < 1e-9 { continue; }
                let u = [(other[0] - p.at[0]) / d, (other[1] - p.at[1]) / d];
                let local = p.local([p.at[0] - u[1], p.at[1] + u[0]]);
                let half = if p.round { dim / 2. } else {
                    (p.size[0] / (2. * local[0].abs())).min(p.size[1] / (2. * local[1].abs()))
                };
                anchors.push((p.at, dim, half, u, d));
            }
            for v in r.vias.iter().filter(|v| v.net == t.net && v.on(t.layer, b.layers) && route::distance(end, v.at) <= b.via[0] / 2.) {
                let d = route::distance(v.at, other);
                if d > 1e-9 { anchors.push((v.at, b.via[0], b.via[0] / 2., [(other[0] - v.at[0]) / d, (other[1] - v.at[1]) / d], d)); }
            }
            for (center, dim, half, u, d) in anchors {
                if t.width >= 0.9 * dim { continue; }
                // Never reach past the track's far end.
                let l = 1_f64.min(1.5 * dim).min(d);
                let at = |x: f64, y: f64| [center[0] + u[0] * x - u[1] * y, center[1] + u[1] * x + u[0] * y];
                let mut points = vec![at(0., 0.9 * half), at(l, t.width / 2.), at(l, -t.width / 2.), at(0., -0.9 * half)];
                if pour::signed_area(&points) < 0. { points.reverse(); }
                if !points.iter().all(|p| place::inside(*p, &b.outline)) { continue; }
                if clear_shape(b, r, t.net, t.layer, &Shape::Region(vec![vec![points.clone()]])) {
                    let drop = Teardrop { points, layer: t.layer, net: t.net };
                    if !r.teardrops.contains(&drop) { r.teardrops.push(drop); }
                }
            }
        }
    }
}
