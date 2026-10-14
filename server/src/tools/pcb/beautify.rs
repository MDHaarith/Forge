use super::{
    drc::Shape,
    route::{distance, segment_distance, Board, Point, Routes, Track, Via},
    tune,
};
use std::collections::{BTreeMap, BTreeSet};

const EPS: f64 = 1e-7;

#[derive(Default)]
pub(super) struct Stats {
    pub before: usize,
    pub after: usize,
    pub vias: usize,
    pub chamfers: usize,
    pub acute: usize,
    pub changed: bool,
}

#[derive(Clone)]
struct Chain {
    points: Vec<Point>,
    track: Track,
}
impl Chain {
    fn tracks(&self) -> Vec<Track> {
        self.points
            .windows(2)
            .filter(|p| distance(p[0], p[1]) > EPS)
            .map(|p| Track {
                a: p[0],
                b: p[1],
                ..self.track.clone()
            })
            .collect()
    }
}

fn key(p: Point) -> (i64, i64) {
    ((p[0] * 1e8).round() as i64, (p[1] * 1e8).round() as i64)
}

fn projection(p: Point, t: &Track) -> Point {
    let d = [t.b[0] - t.a[0], t.b[1] - t.a[1]];
    let l = d[0] * d[0] + d[1] * d[1];
    let f = (((p[0] - t.a[0]) * d[0] + (p[1] - t.a[1]) * d[1]) / l).clamp(0., 1.);
    [t.a[0] + f * d[0], t.a[1] + f * d[1]]
}

fn intersection(a: &Track, b: &Track) -> Option<Point> {
    let u = [a.b[0] - a.a[0], a.b[1] - a.a[1]];
    let v = [b.b[0] - b.a[0], b.b[1] - b.a[1]];
    let cross = u[0] * v[1] - u[1] * v[0];
    if cross.abs() < EPS {
        return None;
    }
    let d = [b.a[0] - a.a[0], b.a[1] - a.a[1]];
    let f = (d[0] * v[1] - d[1] * v[0]) / cross;
    let g = (d[0] * u[1] - d[1] * u[0]) / cross;
    ((-EPS..=1. + EPS).contains(&f) && (-EPS..=1. + EPS).contains(&g))
        .then_some([a.a[0] + f * u[0], a.a[1] + f * u[1]])
}

/// Split before walking: a branch can end in the middle of an earlier committed track.
fn chains(b: &Board, tracks: &[Track], vias: &[Via], net: usize) -> Vec<Chain> {
    let tracks: Vec<_> = tracks
        .iter()
        .filter(|t| t.net == net && distance(t.a, t.b) > EPS)
        .collect();
    let mut anchors = BTreeSet::new();
    let mut edges = vec![];
    let mut seen = BTreeSet::new();
    for t in &tracks {
        let mut points = vec![t.a, t.b];
        for u in tracks.iter().filter(|u| u.layer == t.layer) {
            for p in [u.a, u.b] {
                if segment_distance(p, t.a, t.b) < EPS {
                    points.push(p);
                }
            }
            if let Some(p) = intersection(t, u) {
                points.push(p);
            }
        }
        for p in b
            .pads
            .iter()
            .filter(|p| p.net == net && p.layers & (1 << t.layer) != 0)
        {
            let shape = tune::pad_shape(p, false);
            for at in [t.a, t.b] {
                if shape.contains(at) {
                    anchors.insert((t.layer, key(at)));
                }
            }
            if segment_distance(p.at, t.a, t.b) < EPS {
                points.push(p.at);
                anchors.insert((t.layer, key(p.at)));
            }
        }
        for v in vias
            .iter()
            .chain(&b.fixed_vias)
            .filter(|v| v.net == net && v.on(t.layer, b.layers))
        {
            if segment_distance(v.at, t.a, t.b) < EPS {
                points.push(v.at);
                anchors.insert((t.layer, key(v.at)));
            }
        }
        for f in b
            .fixed
            .iter()
            .filter(|f| f.nets.contains(&net) && f.track.layer == t.layer)
        {
            for p in [f.track.a, f.track.b]
                .into_iter()
                .chain(intersection(t, &f.track))
            {
                if segment_distance(p, t.a, t.b) <= (t.width + f.track.width) / 2. + EPS {
                    let at = projection(p, t);
                    points.push(at);
                    anchors.insert((t.layer, key(at)));
                }
            }
        }
        points.sort_by(|a, c| distance(t.a, *a).total_cmp(&distance(t.a, *c)));
        points.dedup_by(|a, c| distance(*a, *c) < EPS);
        for p in points.windows(2) {
            let (a, c) = (key(p[0]), key(p[1]));
            if seen.insert((t.layer, t.width.to_bits(), a.min(c), a.max(c))) {
                edges.push(Track {
                    a: p[0],
                    b: p[1],
                    ..(*t).clone()
                });
            }
        }
    }
    let mut nodes: BTreeMap<_, Vec<usize>> = BTreeMap::new();
    for (i, t) in edges.iter().enumerate() {
        for p in [t.a, t.b] {
            nodes.entry((t.layer, key(p))).or_default().push(i);
        }
    }
    let stop = |t: &Track, p: Point| {
        anchors.contains(&(t.layer, key(p)))
            || nodes[&(t.layer, key(p))].len() != 2
            || nodes[&(t.layer, key(p))]
                .iter()
                .any(|&i| edges[i].width != t.width)
    };
    let mut used = vec![false; edges.len()];
    let mut result = vec![];
    // Walk anchored chains first, then any closed loops.
    for anchored in [true, false] {
        for (first, t) in edges.iter().enumerate() {
            if used[first] || (anchored && !stop(t, t.a) && !stop(t, t.b)) {
                continue;
            }
            let start = if stop(t, t.b) && !stop(t, t.a) {
                t.b
            } else {
                t.a
            };
            let mut points = vec![start];
            let mut at = start;
            let mut edge = first;
            loop {
                used[edge] = true;
                let u = &edges[edge];
                at = if key(at) == key(u.a) { u.b } else { u.a };
                points.push(at);
                if stop(t, at) {
                    break;
                }
                let Some(next) = nodes[&(t.layer, key(at))]
                    .iter()
                    .copied()
                    .find(|&i| !used[i])
                else {
                    break;
                };
                edge = next;
            }
            let mut merged = vec![];
            for p in points {
                while merged.len() >= 2 {
                    let (a, c): (Point, Point) =
                        (merged[merged.len() - 2], merged[merged.len() - 1]);
                    let u = [c[0] - a[0], c[1] - a[1]];
                    let v = [p[0] - c[0], p[1] - c[1]];
                    if (u[0] * v[1] - u[1] * v[0]).abs() > EPS || u[0] * v[0] + u[1] * v[1] <= 0. {
                        break;
                    }
                    merged.pop();
                }
                merged.push(p);
            }
            result.push(Chain {
                points: merged,
                track: t.clone(),
            });
        }
    }
    // Copper wholly inside a land is redundant, including sub-grid terminal overshoots.
    result.retain(|c| {
        !b.pads.iter().any(|p| {
            p.net == net
                && p.layers & (1 << c.track.layer) != 0
                && c.points
                    .iter()
                    .all(|&q| tune::pad_shape(p, false).contains(q))
        })
    });
    result
}

fn path_length(points: &[Point]) -> f64 {
    points.windows(2).map(|p| distance(p[0], p[1])).sum()
}

fn octilinear(a: Point, c: Point) -> Vec<Vec<Point>> {
    let d = [c[0] - a[0], c[1] - a[1]];
    if d[0].abs() < EPS || d[1].abs() < EPS || (d[0].abs() - d[1].abs()).abs() < EPS {
        return vec![vec![a, c]];
    }
    let diagonal = d[0].abs().min(d[1].abs());
    let v = [diagonal * d[0].signum(), diagonal * d[1].signum()];
    vec![
        vec![a, [a[0] + v[0], a[1] + v[1]], c],
        vec![a, [c[0] - v[0], c[1] - v[1]], c],
    ]
}

fn clean(b: &Board, r: &Routes, track: &Track, points: &[Point]) -> bool {
    points.windows(2).all(|p| {
        tune::clear_track(
            b,
            r,
            &Track {
                a: p[0],
                b: p[1],
                ..track.clone()
            },
        )
    })
}

fn pull(b: &Board, r: &Routes, chain: &mut Chain) {
    let mut i = 0;
    let mut tries = 0;
    // shortcut: 256 pull candidates per chain per round, increase when long maze paths need more polishing.
    while i + 2 < chain.points.len() && tries < 256 {
        let mut replacement = None;
        for j in (i + 2..chain.points.len()).rev() {
            let old = &chain.points[i..=j];
            for path in octilinear(old[0], *old.last().unwrap()) {
                if tries >= 256 {
                    break;
                }
                tries += 1;
                if path == old || path_length(&path) > path_length(old) + EPS {
                    continue;
                }
                if path.len() >= old.len() && path_length(&path) >= path_length(old) - EPS {
                    continue;
                }
                if clean(b, r, &chain.track, &path) {
                    replacement = Some((j, path));
                    break;
                }
                if tries >= 256 {
                    break;
                }
            }
            if replacement.is_some() || tries >= 256 {
                break;
            }
        }
        if let Some((j, path)) = replacement {
            chain.points.splice(i..=j, path);
        }
        i += 1;
    }
}

fn turn(a: Point, c: Point, d: Point) -> f64 {
    let u = [c[0] - a[0], c[1] - a[1]];
    let v = [d[0] - c[0], d[1] - c[1]];
    ((u[0] * v[0] + u[1] * v[1]) / (distance(a, c) * distance(c, d))).clamp(-1., 1.)
}

fn chamfer(b: &Board, r: &Routes, chain: &mut Chain) -> usize {
    let mut count = 0;
    let mut i = 1;
    while i + 1 < chain.points.len() {
        let (a, c, d) = (chain.points[i - 1], chain.points[i], chain.points[i + 1]);
        if turn(a, c, d) > EPS {
            i += 1;
            continue;
        }
        let cut = 1_f64
            .min(3. * chain.track.width)
            .min(0.45 * distance(a, c).min(distance(c, d)));
        let toward = |p: Point| {
            [
                c[0] + (p[0] - c[0]) * cut / distance(p, c),
                c[1] + (p[1] - c[1]) * cut / distance(p, c),
            ]
        };
        let mut chosen = None;
        for path in octilinear(toward(a), toward(d)) {
            let all: Vec<_> = [a]
                .into_iter()
                .chain(path.iter().copied())
                .chain([d])
                .collect();
            if all
                .windows(3)
                .any(|p| turn(p[0], p[1], p[2]) < std::f64::consts::FRAC_1_SQRT_2 - EPS)
            {
                continue;
            }
            if path_length(&all) <= distance(a, c) + distance(c, d) + EPS
                && clean(b, r, &chain.track, &all)
            {
                chosen = Some(path);
                break;
            }
        }
        if let Some(path) = chosen {
            let len = path.len();
            chain.points.splice(i..=i, path);
            count += 1;
            i += len;
        } else {
            i += 1;
        }
    }
    count
}

/// Compare pad connectivity geometrically, including copper contacts and reference planes.
fn connected(b: &Board, tracks: &[Track], vias: &[Via], net: usize) -> bool {
    let mut items: Vec<_> = b.nets[net]
        .pads
        .iter()
        .map(|&i| {
            let p = &b.pads[i];
            (tune::pad_shape(p, false), p.layers)
        })
        .collect();
    let pads = items.len();
    items.extend(
        tracks
            .iter()
            .filter(|t| t.net == net)
            .map(|t| (Shape::Stroke(t.a, t.b, t.width / 2.), 1 << t.layer)),
    );
    items.extend(b.fixed.iter().filter(|f| f.nets.contains(&net)).map(|f| {
        (
            Shape::Stroke(f.track.a, f.track.b, f.track.width / 2.),
            1 << f.track.layer,
        )
    }));
    items.extend(
        vias.iter()
            .chain(&b.fixed_vias)
            .filter(|v| v.net == net)
            .map(|v| (Shape::Disk(v.at, b.via[0] / 2.), v.mask(b.layers))),
    );
    let mut parents: Vec<_> = (0..items.len()).collect();
    let root = |parents: &[usize], mut i: usize| {
        while parents[i] != i {
            i = parents[i];
        }
        i
    };
    let plane = b
        .planes
        .iter()
        .filter(|p| p.0 == net)
        .fold(0, |m, p| m | (1 << p.1));
    for (i, (a, layers)) in items.iter().enumerate() {
        for (j, (c, other)) in items[..i].iter().enumerate() {
            if (layers & other != 0 && a.gap(c) <= EPS)
                || (layers & plane != 0 && other & plane != 0)
            {
                let x = root(&parents, i);
                let y = root(&parents, j);
                parents[x] = y;
            }
        }
    }
    (1..pads).all(|i| root(&parents, i) == root(&parents, 0))
}

fn on_layer(b: &Board, r: &Routes, net: usize, at: Point, layer: usize, omitted: &Via) -> bool {
    b.pads.iter().any(|p| {
        p.net == net && p.layers & (1 << layer) != 0 && tune::pad_shape(p, false).contains(at)
    }) || r
        .vias
        .iter()
        .chain(&b.fixed_vias)
        .any(|v| v != omitted && v.net == net && v.on(layer, b.layers) && distance(v.at, at) < EPS)
        || r.tracks
            .iter()
            .any(|t| t.net == net && t.layer == layer && segment_distance(at, t.a, t.b) < EPS)
}

fn minimize_vias(b: &Board, r: &mut Routes, net: usize) {
    if b.planes.iter().any(|p| p.0 == net) {
        return;
    }
    for via in r
        .vias
        .iter()
        .filter(|v| v.net == net)
        .cloned()
        .collect::<Vec<_>>()
    {
        if b.pads.iter().any(|p| {
            p.net == net && p.drill.is_some() && tune::pad_shape(p, false).contains(via.at)
        }) {
            continue;
        }
        let mut vias = r.vias.clone();
        vias.retain(|v| *v != via);
        if connected(b, &r.tracks, &vias, net) {
            r.vias = vias;
            continue;
        }
        let paths = chains(b, &r.tracks, &r.vias, net);
        let sides: Vec<_> = paths
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                if distance(c.points[0], via.at) < EPS {
                    Some((i, *c.points.last().unwrap()))
                } else if distance(*c.points.last().unwrap(), via.at) < EPS {
                    Some((i, c.points[0]))
                } else {
                    None
                }
            })
            .collect();
        // shortcut: relocate only two-branch vias, add branch-layer reassignment when junction vias dominate boards.
        let [(i, a), (j, c)] = sides.as_slice() else {
            continue;
        };
        let old = path_length(&paths[*i].points) + path_length(&paths[*j].points);
        let mut chosen = None;
        for layer in [paths[*i].track.layer, paths[*j].track.layer] {
            if !on_layer(b, r, net, *a, layer, &via) || !on_layer(b, r, net, *c, layer, &via) {
                continue;
            }
            let track = Track {
                layer,
                ..paths[*i].track.clone()
            };
            for points in octilinear(*a, *c) {
                if path_length(&points) > old + EPS || !clean(b, r, &track, &points) {
                    continue;
                }
                let mut tracks: Vec<_> = paths
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| k != i && k != j)
                    .flat_map(|(_, c)| c.tracks())
                    .collect();
                tracks.extend(
                    Chain {
                        points,
                        track: track.clone(),
                    }
                    .tracks(),
                );
                tracks.extend(r.tracks.iter().filter(|t| t.net != net).cloned());
                if connected(b, &tracks, &vias, net) {
                    chosen = Some(tracks);
                    break;
                }
            }
            if chosen.is_some() {
                break;
            }
        }
        if let Some(tracks) = chosen {
            r.tracks = tracks;
            r.vias = vias;
        }
    }
}

fn pad_entries(b: &Board, r: &Routes, chain: &mut Chain) {
    for reverse in [false, true] {
        if reverse {
            chain.points.reverse();
        }
        let end = chain.points[0];
        if let Some(pad) = b.pads.iter().find(|p| {
            p.net == chain.track.net
                && p.drill.is_none()
                && p.layers & (1 << chain.track.layer) != 0
                && tune::pad_shape(p, false).contains(end)
        }) {
            let other = chain.points[1];
            let delta = [other[0] - end[0], other[1] - end[1]];
            let oct = delta[0].abs() < EPS
                || delta[1].abs() < EPS
                || (delta[0].abs() - delta[1].abs()).abs() < EPS;
            if distance(end, pad.at) > EPS || (!oct && distance(end, other) > 0.6) {
                let axis = if pad.size[0] >= pad.size[1] { 0 } else { 1 };
                let mut local = [0.; 2];
                local[axis] = pad.local(other)[axis].signum() * 0.5_f64.min(pad.size[axis] / 2.);
                let entry = pad.point(local);
                let mut candidates = octilinear(pad.at, other);
                candidates.extend(
                    octilinear(entry, other)
                        .into_iter()
                        .map(|path| [pad.at].into_iter().chain(path).collect()),
                );
                for points in candidates {
                    if path_length(&points) <= distance(end, other) + 0.2 + EPS
                        && clean(b, r, &chain.track, &points)
                    {
                        chain.points.splice(0..=1, points);
                        break;
                    }
                }
            }
        }
        if reverse {
            chain.points.reverse();
        }
    }
}

pub(super) fn finish(b: &Board, r: &mut Routes) -> Stats {
    // shortcut: no channel centring, add it when pulling leaves visibly off-centre parallel runs.
    let original = r.clone();
    let mut stats = Stats {
        before: r.tracks.len(),
        ..Stats::default()
    };
    for net in 0..b.nets.len() {
        if b.pairs.iter().any(|p| p.p == net || p.n == net) {
            continue;
        }
        let old_tracks: Vec<_> = r.tracks.iter().filter(|t| t.net == net).cloned().collect();
        let old_vias: Vec<_> = r.vias.iter().filter(|v| v.net == net).cloned().collect();
        if old_tracks.is_empty() || !connected(b, &old_tracks, &old_vias, net) {
            continue;
        }
        let limit = tune::length(&old_tracks, net) + 0.2;
        let mut corners = 0;
        // shortcut: four deterministic polishing rounds, increase when another round measurably improves dense boards.
        for _ in 0..4 {
            let before_tracks = r.tracks.clone();
            let before_vias = r.vias.clone();
            let mut round_corners = 0;
            let mut paths = chains(b, &r.tracks, &r.vias, net);
            for path in &mut paths {
                pull(b, r, path);
                round_corners += chamfer(b, r, path);
            }
            r.tracks.retain(|t| t.net != net);
            r.tracks.extend(paths.iter().flat_map(Chain::tracks));
            minimize_vias(b, r, net);
            let mut paths = chains(b, &r.tracks, &r.vias, net);
            for path in &mut paths {
                pad_entries(b, r, path);
            }
            r.tracks.retain(|t| t.net != net);
            r.tracks.extend(paths.iter().flat_map(Chain::tracks));
            if !connected(b, &r.tracks, &r.vias, net) || tune::length(&r.tracks, net) > limit + EPS
            {
                r.tracks = before_tracks;
                r.vias = before_vias;
                break;
            }
            corners += round_corners;
            if r.tracks == before_tracks && r.vias == before_vias {
                break;
            }
        }
        // Keep the original copper if a pad contact or a fixed obstacle defeats normalization.
        if r.tracks
            .iter()
            .filter(|t| t.net == net)
            .any(|t| !tune::clear_track(b, r, t))
        {
            r.tracks.retain(|t| t.net != net);
            r.tracks.extend(old_tracks);
            r.vias.retain(|v| v.net != net);
            r.vias.extend(old_vias);
        } else {
            stats.chamfers += corners;
        }
        stats.acute += chains(b, &r.tracks, &r.vias, net)
            .iter()
            .map(|c| {
                c.points
                    .windows(3)
                    .filter(|p| turn(p[0], p[1], p[2]) < -EPS)
                    .count()
            })
            .sum::<usize>();
    }
    stats.after = r.tracks.len();
    stats.vias = original.vias.len() - r.vias.len();
    let signature = |tracks: &[Track]| {
        let mut keys: Vec<_> = tracks
            .iter()
            .map(|t| {
                let (a, c) = (key(t.a), key(t.b));
                (t.net, t.layer, t.width.to_bits(), a.min(c), a.max(c))
            })
            .collect();
        keys.sort_unstable();
        keys
    };
    stats.changed = signature(&r.tracks) != signature(&original.tracks) || r.vias != original.vias;
    if !stats.changed {
        r.tracks = original.tracks;
    }
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (super::super::PreparedBoard, Board) {
        let prepared = super::super::prepare(
            &json!({"board":{"width":30,"height":20},"parts":[
            {"ref":"R1","kind":"resistor","footprint":"0603","at":[-8,0]},
            {"ref":"R2","kind":"resistor","footprint":"0603","at":[8,0]}],
            "nets":{"N":["R1.2","R2.1"]}})
            .to_string(),
        )
        .unwrap();
        let board = Board::from_prepared(&prepared).unwrap();
        (prepared, board)
    }

    fn routes(tracks: Vec<Track>, vias: Vec<Via>) -> Routes {
        Routes {
            tracks,
            vias,
            routed: vec![true],
            passes: 1,
            teardrops: vec![],
            fallback: vec![],
        }
    }
    fn track(a: Point, b: Point, layer: usize) -> Track {
        Track {
            a,
            b,
            layer,
            width: 0.2,
            net: 0,
        }
    }

    #[test]
    fn normalization_removes_overlaps_and_keeps_a_mid_segment_branch() {
        let (_, mut b) = fixture();
        let a = b.pads[b.nets[0].pads[0]].at;
        let c = b.pads[b.nets[0].pads[1]].at;
        let mut pad = b.pads[b.nets[0].pads[0]].clone();
        pad.at = [0., 5.];
        pad.size = [0.3; 2];
        pad.name = "P3.1".into();
        b.nets[0].pads.push(b.pads.len());
        b.pads.push(pad);
        let mut r = routes(
            vec![
                track(a, c, 0),
                track(a, c, 0),
                track(a, [0., 0.], 0),
                track([0., 0.], [0., 0.], 0),
                track([0., 0.], [0., 5.], 0),
            ],
            vec![],
        );
        let before = tune::length(&r.tracks, 0);
        finish(&b, &mut r);
        assert_eq!(r.tracks.len(), 3);
        assert!(tune::length(&r.tracks, 0) < before);
        assert!(connected(&b, &r.tracks, &r.vias, 0));
        assert_eq!(super::super::route::clearance_check(&b, &r).0, 0);
    }

    #[test]
    fn clean_right_and_acute_corners_become_45_degree_turns() {
        let (_, b) = fixture();
        for end in [[0., 4.], [-3., 4.]] {
            let mut c = Chain {
                points: vec![[-3., 1.], [0., 1.], end],
                track: track([-3., 1.], end, 0),
            };
            let before = path_length(&c.points);
            let r = routes(c.tracks(), vec![]);
            assert_eq!(chamfer(&b, &r, &mut c), 1);
            assert!(c
                .points
                .windows(3)
                .all(|p| turn(p[0], p[1], p[2]) >= std::f64::consts::FRAC_1_SQRT_2 - EPS));
            assert!(path_length(&c.points) < before);
            assert!(clean(&b, &r, &c.track, &c.points));
        }
    }

    #[test]
    fn two_unneeded_layer_changes_are_removed_without_losing_pads() {
        let (prepared, b) = fixture();
        let a = b.pads[b.nets[0].pads[0]].at;
        let c = b.pads[b.nets[0].pads[1]].at;
        let mut r = routes(
            vec![
                track(a, [-2., 0.], 0),
                track([-2., 0.], [2., 0.], 1),
                track([2., 0.], c, 0),
            ],
            vec![
                Via {
                    at: [-2., 0.],
                    net: 0,
                    span: (0, 1),
                },
                Via {
                    at: [2., 0.],
                    net: 0,
                    span: (0, 1),
                },
            ],
        );
        assert_eq!(super::super::route::clearance_check(&b, &r).0, 0);
        let before = tune::length(&r.tracks, 0);
        let stats = finish(&b, &mut r);
        assert_eq!(stats.vias, 2);
        assert!(r.vias.is_empty());
        assert!(tune::length(&r.tracks, 0) <= before + EPS);
        assert!(connected(&b, &r.tracks, &r.vias, 0));
        assert_eq!(super::super::route::clearance_check(&b, &r).0, 0);
        let circuit = super::super::circuit(&prepared, &b, &r, &vec![]);
        let spec = json!({"board":{"width":30,"height":20},"parts":[
            {"ref":"R1","kind":"resistor","footprint":"0603","at":[-8,0]},
            {"ref":"R2","kind":"resistor","footprint":"0603","at":[8,0]}],"nets":{"N":["R1.2","R2.1"]}});
        assert!(
            super::super::drc::check(&spec.to_string(), &circuit.to_string())
                .unwrap()
                .ends_with("No DRC errors.")
        );
    }
}
