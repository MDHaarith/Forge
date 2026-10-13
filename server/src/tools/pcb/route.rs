use super::{antenna, place, Layer, PreparedBoard};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap},
};

pub type Point = [f64; 2];
#[derive(Clone, Debug)]
pub struct Pad {
    pub name: String,
    pub at: Point,
    pub size: Point,
    pub rotation: f64,
    pub round: bool,
    pub layers: u8,
    pub drill: Option<[f64; 2]>,
    pub plated: bool,
    pub net: usize,
}
#[derive(Clone, Debug)]
pub struct Net {
    pub name: String,
    pub pads: Vec<usize>,
    pub width: f64,
    pub clearance: f64,
}
#[derive(Clone, Debug)]
pub struct Board {
    pub size: Point,
    pub pads: Vec<Pad>,
    pub outline: Vec<Point>,
    pub layers: usize,
    pub origin: Point,
    pub fixed: Vec<FixedTrace>,
    pub antenna_keepouts: Vec<Vec<Point>>,
    pub nets: Vec<Net>,
    pub holes: Vec<(Point, f64)>,
    pub keepouts: Vec<(Point, Point, u8)>,
    pub clearance: f64,
    pub edge: f64,
    pub via: [f64; 2],
    pub via_in_pad: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub a: Point,
    pub b: Point,
    pub layer: usize,
    pub width: f64,
    pub net: usize,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Via {
    pub at: Point,
    pub net: usize,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Routes {
    pub tracks: Vec<Track>,
    pub vias: Vec<Via>,
    pub routed: Vec<bool>,
    pub passes: usize,
}

pub fn distance(a: Point, b: Point) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
pub fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l = d[0] * d[0] + d[1] * d[1];
    let t = if l == 0.0 {
        0.0
    } else {
        ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l
    }
    .clamp(0.0, 1.0);
    distance(p, [a[0] + t * d[0], a[1] + t * d[1]])
}
pub fn rect_distance(p: Point, c: Point, size: Point) -> f64 {
    let dx = ((p[0] - c[0]).abs() - size[0] / 2.0).max(0.0);
    let dy = ((p[1] - c[1]).abs() - size[1] / 2.0).max(0.0);
    dx.hypot(dy)
}

#[derive(Clone, Debug)]
pub struct FixedTrace {
    pub track: Track,
    pub nets: Vec<usize>,
    pub part: usize,
}

impl Layer {
    pub fn index(self) -> usize {
        match self {
            Self::Top => 0,
            Self::Bottom => 1,
            Self::Inner1 => 2,
            Self::Inner2 => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Inner1 => "inner1",
            Self::Inner2 => "inner2",
        }
    }
    pub fn from_index(index: usize) -> Self {
        [Self::Top, Self::Bottom, Self::Inner1, Self::Inner2][index]
    }
}

impl Pad {
    pub fn local(&self, p: Point) -> Point {
        let (s, c) = self.rotation.to_radians().sin_cos();
        let [x, y] = [p[0] - self.at[0], p[1] - self.at[1]];
        [c * x + s * y, -s * x + c * y]
    }
    pub fn point(&self, p: Point) -> Point {
        let (s, c) = self.rotation.to_radians().sin_cos();
        [
            self.at[0] + c * p[0] - s * p[1],
            self.at[1] + s * p[0] + c * p[1],
        ]
    }
    pub fn bounds(&self) -> Point {
        let (s, c) = self.rotation.to_radians().sin_cos();
        [
            c.abs() * self.size[0] + s.abs() * self.size[1],
            s.abs() * self.size[0] + c.abs() * self.size[1],
        ]
    }
    pub fn effective_size(&self) -> Point {
        let (_s, c) = self.rotation.to_radians().sin_cos();
        if c.abs() < 1e-6 {
            [self.size[1], self.size[0]]
        } else {
            [self.size[0], self.size[1]]
        }
    }
}

pub fn obround_distance(p: Point, size: Point) -> f64 {
    if size[1] >= size[0] {
        let half = (size[1] - size[0]) / 2.0;
        let cy = p[1].clamp(-half, half);
        p[0].hypot(p[1] - cy) - size[0] / 2.0
    } else {
        let half = (size[0] - size[1]) / 2.0;
        let cx = p[0].clamp(-half, half);
        (p[0] - cx).hypot(p[1]) - size[1] / 2.0
    }
}

impl Board {
    pub fn clearance(&self, net: usize) -> f64 {
        self.nets
            .get(net)
            .map(|n| n.clearance)
            .unwrap_or(self.clearance)
    }
    pub fn from_prepared(prepared: &PreparedBoard) -> Result<Self, String> {
        let spec = &prepared.spec;
        let bounds = spec.board.bounds();
        let grid = (spec.rules.clearance / 2.).max(0.05);
        let cells = (((bounds[2] - bounds[0]) / grid).floor() + 1.)
            * (((bounds[3] - bounds[1]) / grid).floor() + 1.)
            * spec.board.layers as f64;
        // shortcut: eight million grid cells, upgrade to sparse or shape-based routing for larger boards.
        if !cells.is_finite() || cells > 8_000_000. {
            return Err("board is too large for the routing grid".into());
        }
        let mut b = Self {
            size: [bounds[2] - bounds[0], bounds[3] - bounds[1]],
            origin: [bounds[0], bounds[1]],
            outline: spec.board.polygon(),
            layers: spec.board.layers,
            pads: vec![],
            nets: vec![],
            fixed: vec![],
            antenna_keepouts: vec![],
            holes: spec.holes.iter().map(|h| (h.at, h.d)).collect(),
            keepouts: spec
                .keepouts
                .iter()
                .map(|k| {
                    (
                        [k.rect[0], k.rect[1]],
                        [k.rect[2], k.rect[3]],
                        k.layers.iter().fold(0, |mask, l| mask | (1 << l.index())),
                    )
                })
                .collect(),
            clearance: spec.rules.clearance,
            edge: spec.rules.edge,
            via: spec.rules.via,
            via_in_pad: spec.rules.via_in_pad,
        };
        for p in &prepared.parts {
            for pad in &p.footprint.pads {
                let layers = if !pad.plated {
                    0
                } else if pad.drill.is_some() {
                    (1 << b.layers) - 1
                } else {
                    1 << p.side.index()
                };
                b.pads.push(Pad {
                    name: format!("{}.{}", spec.parts[p.part].reference, pad.number),
                    // KiCad footprints have y down; the board and every output have y up.
                    at: p.point([pad.at[0], -pad.at[1]]),
                    size: pad.size,
                    rotation: p.rotation,
                    round: pad.round,
                    drill: pad.drill,
                    layers,
                    plated: pad.plated,
                    net: usize::MAX,
                });
            }
        }
        for n in &prepared.nets {
            let pads: Vec<_> = n
                .pins
                .iter()
                .flat_map(|pin| {
                    let name = format!("{}.{}", spec.parts[pin.part].reference, pin.number);
                    b.pads
                        .iter()
                        .enumerate()
                        .filter(move |(_, p)| p.name == name)
                        .map(|(i, _)| i)
                })
                .collect();
            for &i in &pads {
                b.pads[i].net = b.nets.len();
            }
            b.nets.push(Net {
                name: n.name.clone(),
                pads,
                width: n.trace,
                clearance: spec.clearance(&n.name),
            });
        }
        // A pour can declare a net without terminals (for example an antenna reference plane).
        for pour in &spec.pours {
            if !b.nets.iter().any(|n| n.name == pour.net) {
                b.nets.push(Net {
                    name: pour.net.clone(),
                    pads: vec![],
                    width: spec.rules.trace,
                    clearance: spec.clearance(&pour.net),
                });
            }
        }
        for p in &prepared.parts {
            if spec.parts[p.part].kind != super::Kind::Antenna {
                continue;
            }
            let geometry = antenna::geometry(spec.parts[p.part].shape.as_deref().unwrap_or(""))?;
            let nets: Vec<_> = b
                .pads
                .iter()
                .filter(|pad| {
                    pad.name
                        .starts_with(&format!("{}.", spec.parts[p.part].reference))
                })
                .map(|p| p.net)
                .filter(|n| *n != usize::MAX)
                .collect();
            for pair in geometry.route.windows(2) {
                b.fixed.push(FixedTrace {
                    track: Track {
                        a: p.point(pair[0]),
                        b: p.point(pair[1]),
                        layer: p.side.index(),
                        width: geometry.width,
                        net: nets.first().copied().unwrap_or(usize::MAX),
                    },
                    nets: nets.clone(),
                    part: p.part,
                });
            }
            for (at, size) in geometry.keepouts() {
                let corners = super::pour::rect(at, size)
                    .into_iter()
                    .map(|q| p.point(q))
                    .collect::<Vec<_>>();
                // Keep rotated antenna exclusions as polygons, never an expanded bounding box.
                b.antenna_keepouts.push(corners);
            }
        }
        Ok(b)
    }
}

struct Grid {
    fanouts: Vec<Track>,
    nx: usize,
    ny: usize,
    g: f64,
    origin: Point,
    layers: usize,
}
impl Grid {
    fn new(b: &Board) -> Self {
        let g = (b.clearance / 2.0).max(0.05);
        let mut centers: BTreeMap<&str, (Point, usize)> = BTreeMap::new();
        for p in &b.pads {
            let (sum, count) = centers
                .entry(p.name.split('.').next().unwrap())
                .or_default();
            sum[0] += p.at[0];
            sum[1] += p.at[1];
            *count += 1;
        }
        let fanouts = b
            .pads
            .iter()
            .filter(|p| p.drill.is_none() && p.net != usize::MAX)
            .filter_map(|p| {
                let (sum, count) = centers[p.name.split('.').next().unwrap()];
                let center = p.local([sum[0] / count as f64, sum[1] / count as f64]);
                let axis = if p.size[0] >= p.size[1] { 0 } else { 1 };
                if center[axis].abs() < 1e-9 {
                    return None;
                }
                let mut end = [0.; 2];
                end[axis] = -center[axis].signum()
                    * (p.size[axis] / 2. + b.via[0] / 2. + b.clearance(p.net) + g);
                Some(Track {
                    a: p.at,
                    b: p.point(end),
                    layer: p.layers.trailing_zeros() as usize,
                    width: b.nets[p.net].width,
                    net: p.net,
                })
            })
            .collect();
        Self {
            fanouts,
            nx: (b.size[0] / g).floor() as usize + 1,
            ny: (b.size[1] / g).floor() as usize + 1,
            g,
            origin: b.origin,
            layers: b.layers,
        }
    }
    fn id(&self, x: usize, y: usize, z: usize) -> usize {
        z * self.nx * self.ny + y * self.nx + x
    }
    fn xyz(&self, i: usize) -> (usize, usize, usize) {
        (
            i % self.nx,
            (i / self.nx) % self.ny,
            i / (self.nx * self.ny),
        )
    }
    fn at(&self, i: usize) -> Point {
        let (x, y, _) = self.xyz(i);
        [
            x as f64 * self.g + self.origin[0],
            y as f64 * self.g + self.origin[1],
        ]
    }
    fn node(&self, p: Point, z: usize) -> usize {
        self.id(
            ((p[0] - self.origin[0]) / self.g).round() as usize,
            ((p[1] - self.origin[1]) / self.g).round() as usize,
            z,
        )
    }
    fn mark(
        &self,
        blocked: &mut [bool],
        c: Point,
        size: Point,
        layers: u8,
        within: impl Fn(Point) -> bool,
    ) {
        self.visit(c, size, layers, within, |i| blocked[i] = true);
    }
    fn visit(
        &self,
        c: Point,
        size: Point,
        layers: u8,
        within: impl Fn(Point) -> bool,
        mut hit: impl FnMut(usize),
    ) {
        let low = |v: f64, axis: usize| {
            (((v - self.origin[axis]) / self.g).floor() as isize).max(0) as usize
        };
        let high = |v: f64, axis: usize| {
            (((v - self.origin[axis]) / self.g).ceil() as isize).max(0) as usize
        };
        let x0 = low(c[0] - size[0] / 2.0, 0).min(self.nx);
        let y0 = low(c[1] - size[1] / 2.0, 1).min(self.ny);
        let x1 = high(c[0] + size[0] / 2.0, 0).min(self.nx - 1);
        let y1 = high(c[1] + size[1] / 2.0, 1).min(self.ny - 1);
        for z in 0..self.layers {
            if layers & (1 << z) == 0 {
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let i = self.id(x, y, z);
                    if within(self.at(i)) {
                        hit(i);
                    }
                }
            }
        }
    }
    fn obstacles(
        &self,
        b: &Board,
        r: &Routes,
        net: usize,
        radius: f64,
        segments: bool,
        reserve: bool,
    ) -> Vec<bool> {
        let mut blocked = vec![false; self.nx * self.ny * self.layers];
        // Cover the complete segment, including diagonal negotiation moves.
        let guard = if !segments { 0. } else if reserve { self.g / 2.0 } else { self.g / 2_f64.sqrt() };
        let margin = b.edge + radius + guard;
        let rectangular = b.outline.len() == 4
            && b.outline.iter().all(|p| {
                (p[0] == b.origin[0] || p[0] == b.origin[0] + b.size[0])
                    && (p[1] == b.origin[1] || p[1] == b.origin[1] + b.size[1])
            });
        for (i, v) in blocked.iter_mut().enumerate() {
            let p = self.at(i);
            *v = if rectangular {
                p[0] < b.origin[0] + margin
                    || p[1] < b.origin[1] + margin
                    || p[0] > b.origin[0] + b.size[0] - margin
                    || p[1] > b.origin[1] + b.size[1] - margin
            } else {
                !place::inside(p, &b.outline)
                    || b.outline
                        .iter()
                        .zip(b.outline.iter().cycle().skip(1))
                        .any(|(a, bb)| segment_distance(p, *a, *bb) < margin)
            };
        }
        let c = b.clearance(net) + radius + guard;
        for p in &b.pads {
            if p.net == net {
                continue;
            }
            let clearance = b.clearance(net).max(b.clearance(p.net));
            // Square expansion is conservative along an entire orthogonal edge, without an extra half-cell pad penalty.
            let pad_c = if p.round || p.rotation.rem_euclid(90.).abs() > 1e-9 {
                clearance + radius + guard
            } else {
                clearance + radius
            };
            let ext = [p.bounds()[0] + 2.0 * pad_c, p.bounds()[1] + 2.0 * pad_c];
            let layers = if !p.plated {
                (1 << self.layers) - 1
            } else {
                p.layers
            };
            self.mark(&mut blocked, p.at, ext, layers, |q| {
                if !p.plated {
                    p.drill
                        .is_some_and(|d| obround_distance(p.local(q), d) < pad_c)
                } else if p.round {
                    obround_distance(p.local(q), p.size) < pad_c
                } else if !segments {
                    rect_distance(p.local(q), [0.; 2], p.size) < pad_c
                } else {
                    p.local(q)[0].abs() < p.size[0] / 2.0 + pad_c
                        && p.local(q)[1].abs() < p.size[1] / 2.0 + pad_c
                }
            });
        }
        for &(at, d) in &b.holes {
            let radius = d / 2.0 + c;
            self.mark(
                &mut blocked,
                at,
                [radius * 2.0; 2],
                (1 << self.layers) - 1,
                |q| distance(q, at) < radius,
            );
        }
        for &(at, size, layers) in &b.keepouts {
            self.mark(
                &mut blocked,
                at,
                [
                    size[0] + 2.0 * (radius + guard),
                    size[1] + 2.0 * (radius + guard),
                ],
                layers,
                |q| rect_distance(q, at, size) < radius + guard,
            );
        }
        for polygon in &b.antenna_keepouts {
            let bounds = polygon.iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |mut v, p| {
                    v[0] = v[0].min(p[0]);
                    v[1] = v[1].min(p[1]);
                    v[2] = v[2].max(p[0]);
                    v[3] = v[3].max(p[1]);
                    v
                },
            );
            self.mark(
                &mut blocked,
                [(bounds[0] + bounds[2]) / 2., (bounds[1] + bounds[3]) / 2.],
                [
                    bounds[2] - bounds[0] + 2. * (radius + guard),
                    bounds[3] - bounds[1] + 2. * (radius + guard),
                ],
                (1 << self.layers) - 1,
                |q| {
                    place::inside(q, polygon)
                        || polygon
                            .iter()
                            .zip(polygon.iter().cycle().skip(1))
                            .any(|(a, bb)| segment_distance(q, *a, *bb) < radius + guard)
                },
            );
        }
        // Reserve short SMD exits so earlier nets cannot trap later pads before they can place a via.
        for t in self.fanouts.iter().filter(|_| reserve).chain(
            b.fixed
                .iter()
                .filter(|f| !f.nets.contains(&net))
                .map(|f| &f.track),
        ) {
            if t.net == net && t.net != usize::MAX {
                continue;
            }
            self.track(b, t, net, radius + guard, |i| blocked[i] = true);
        }
        self.copper(b, r, net, radius + guard, |i| blocked[i] = true);
        blocked
    }
    fn track(&self, b: &Board, t: &Track, net: usize, radius: f64, hit: impl FnMut(usize)) {
        let radius = t.width / 2.0 + radius + b.clearance(net).max(b.clearance(t.net));
        let center = [(t.a[0] + t.b[0]) / 2.0, (t.a[1] + t.b[1]) / 2.0];
        let size = [
            (t.a[0] - t.b[0]).abs() + 2.0 * radius,
            (t.a[1] - t.b[1]).abs() + 2.0 * radius,
        ];
        self.visit(center, size, 1 << t.layer, |q| segment_distance(q, t.a, t.b) < radius, hit);
    }
    /// Visits every cell too close to another net's routed copper.
    fn copper(&self, b: &Board, r: &Routes, net: usize, radius: f64, mut hit: impl FnMut(usize)) {
        for t in r.tracks.iter().filter(|t| t.net != net) {
            self.track(b, t, net, radius, &mut hit);
        }
        for v in r.vias.iter().filter(|v| v.net != net) {
            let radius = b.via[0] / 2.0 + radius + b.clearance(net).max(b.clearance(v.net));
            self.visit(
                v.at,
                [radius * 2.0; 2],
                (1 << self.layers) - 1,
                |q| distance(q, v.at) < radius,
                &mut hit,
            );
        }
    }
    /// Counts how many other-net copper halos cover each cell, for trace and via moves.
    fn congestion<'a>(&self, b: &Board, others: &Routes, net: usize, history: &'a [u32], present: u32) -> Congestion<'a> {
        let mut trace = vec![0u32; history.len()];
        let mut via = vec![0u32; history.len()];
        let mut seen = vec![false; history.len()];
        let mut touched = vec![];
        for n in 0..b.nets.len() {
            if n == net { continue; }
            let own = Routes {
                tracks: others.tracks.iter().filter(|t| t.net == n).cloned().collect(),
                vias: others.vias.iter().filter(|v| v.net == n).cloned().collect(),
                routed: vec![], passes: 0,
            };
            for (counts, radius) in [(&mut trace, b.nets[net].width / 2.0), (&mut via, b.via[0] / 2.0)] {
                self.copper(b, &own, net, radius, |i| {
                    if !seen[i] {
                        seen[i] = true;
                        touched.push(i);
                        counts[i] += 1;
                    }
                });
                for i in touched.drain(..) { seen[i] = false; }
            }
        }
        Congestion { trace, via, history, present }
    }

    fn masks(&self, b: &Board, r: &Routes, net: usize, reserve: bool) -> Masks {
        let blocked = self.obstacles(b, r, net, b.nets[net].width / 2.0, true, reserve);
        let mut via_blocked = self.obstacles(b, r, net, b.via[0] / 2.0, false, reserve);
        // Open vias stay outside SMD copper; filled vias may overlap only their own net.
        for p in b.pads.iter().filter(|p| p.drill.is_none() && !(b.via_in_pad && p.net == net)) {
            let margin = b.via[0] / 2.0 + if reserve { b.clearance(net).max(b.clearance(p.net)) } else { 0. };
            self.mark(
                &mut via_blocked,
                p.at,
                [p.bounds()[0] + 2. * margin, p.bounds()[1] + 2. * margin],
                (1 << b.layers) - 1,
                |q| rect_distance(p.local(q), [0.; 2], p.size) <= margin + 1e-9,
            );
        }
        (blocked, via_blocked)
    }

    /// Cells of `path` that other nets' copper also claims.
    fn shared(&self, c: &Congestion, path: &[usize]) -> Vec<usize> {
        let mut shared = vec![];
        for pair in path.windows(2) {
            let (x, y, z) = self.xyz(pair[1]);
            if z != self.xyz(pair[0]).2 {
                if (0..self.layers).any(|l| c.via[self.id(x, y, l)] > 0) {
                    shared.extend((0..self.layers).map(|l| self.id(x, y, l)));
                }
            } else if c.trace[pair[1]] > 0 {
                shared.push(pair[1]);
            }
        }
        shared
    }

    // shortcut: grid maze router, upgrade to a shape-based router when real boards leave nets unrouted.
    #[allow(clippy::too_many_arguments)]
    fn astar(
        &self,
        b: &Board,
        r: &Routes,
        net: usize,
        a: usize,
        bb: usize,
        tree: &[usize],
        (blocked, via_blocked): &Masks,
        congestion: Option<&Congestion>,
    ) -> Option<Vec<usize>> {
        let shared_via = |c: &Congestion, x, y| (0..self.layers).map(|l| c.via[self.id(x, y, l)]).sum::<u32>();
        let source = &b.pads[a];
        let target = &b.pads[bb];
        let target_xy = self.xyz(self.node(target.at, 0));
        let free_layer_change = b.pads.iter().any(|p| p.net == net && p.drill.is_some())
            || r.vias.iter().any(|v| v.net == net);
        let heuristic = |i| {
            let (x, y, z) = self.xyz(i);
            let (dx, dy) = (x.abs_diff(target_xy.0), y.abs_diff(target_xy.1));
            ((dx + dy) * 10 - if congestion.is_some() { dx.min(dy) * 6 } else { 0 }) as u32
                + if !free_layer_change && target.layers & (1 << z) == 0 {
                    300
                } else {
                    0
                }
        };
        let mut cost = vec![u32::MAX; blocked.len()];
        let mut parent = vec![usize::MAX; blocked.len()];
        let mut queue = BinaryHeap::new();
        let starts: Vec<_> = if tree.is_empty() {
            (0..self.layers)
                .filter(|z| source.layers & (1 << z) != 0)
                .map(|z| self.node(source.at, z))
                .collect()
        } else {
            tree.to_vec()
        };
        for i in starts {
            if !blocked[i] && cost[i] != 0 {
                cost[i] = 0;
                queue.push(Reverse((heuristic(i), Reverse(0), i)));
            }
        }
        while let Some(Reverse((_, Reverse(g), i))) = queue.pop() {
            if cost[i] != g {
                continue;
            }
            let (x, y, z) = self.xyz(i);
            if x == target_xy.0 && y == target_xy.1 && target.layers & (1 << z) != 0 {
                let mut path = vec![i];
                let mut at = i;
                while parent[at] != usize::MAX {
                    at = parent[at];
                    path.push(at);
                }
                path.reverse();
                return Some(path);
            }
            for (dx, dy, nz) in [(1, 0, z), (-1, 0, z), (0, 1, z), (0, -1, z),
                (1, 1, z), (1, -1, z), (-1, 1, z), (-1, -1, z)]
                .into_iter()
                .chain((0..self.layers).filter(|l| *l != z).map(|l| (0, 0, l)))
            {
                let xx = x as isize + dx;
                let yy = y as isize + dy;
                if xx < 0 || yy < 0 || xx >= self.nx as isize || yy >= self.ny as isize {
                    continue;
                }
                let diagonal = dx != 0 && dy != 0;
                if diagonal && (congestion.is_none()
                    || blocked[self.id(xx as usize, y, z)]
                    || blocked[self.id(x, yy as usize, z)])
                {
                    continue;
                }
                let is_via = dx == 0 && dy == 0;
                let next = self.id(xx as usize, yy as usize, nz);
                if blocked[next]
                    || (is_via && (0..self.layers).any(|l| via_blocked[self.id(x, y, l)]))
                {
                    continue;
                }
                // Existing same-net PTH pads connect layers without inserting a second drill.
                let through = is_via
                    && (b.pads.iter().any(|p| {
                        p.layers == (1 << b.layers) - 1
                            && p.net == net
                            && p.drill
                                .is_some_and(|d| obround_distance(p.local(self.at(i)), d) <= 0.)
                    }) || r
                        .vias
                        .iter()
                        .any(|v| v.net == net && distance(self.at(i), v.at) < 1e-9));
                let base = if diagonal {
                    14
                } else if !is_via {
                    10
                } else if through {
                    0
                } else {
                    300
                };
                // PathFinder: other nets' copper is a cost, not a wall: (base + history) * present.
                let step = congestion.map_or(base, |c| {
                    let shared = if is_via { shared_via(c, x, y) } else { c.trace[next] };
                    let history = if is_via {
                        (0..self.layers).map(|l| c.history[self.id(x, y, l)]).max().unwrap_or(0)
                    } else { c.history[next] };
                    (base + history).saturating_mul(1 + c.present.saturating_mul(shared))
                });
                let nc = g.saturating_add(step);
                if nc < cost[next] {
                    cost[next] = nc;
                    parent[next] = i;
                    queue.push(Reverse((nc.saturating_add(heuristic(next)), Reverse(nc), next)));
                }
            }
        }
        None
    }
    fn commit(&self, b: &Board, r: &mut Routes, net: usize, a: Point, bb: Point, path: &[usize]) {
        let mut points = vec![(a, self.xyz(path[0]).2)];
        points.extend(path.iter().map(|&i| (self.at(i), self.xyz(i).2)));
        points.push((bb, self.xyz(*path.last().unwrap()).2));
        for pair in points.windows(2) {
            let ((a, z), (bb, zz)) = (pair[0], pair[1]);
            if z != zz {
                let through = b.pads.iter().any(|p| {
                    p.layers == (1 << b.layers) - 1
                        && p.net == net
                        && p.drill
                            .is_some_and(|d| obround_distance(p.local(a), d) <= 0.)
                });
                if !through
                    && !r
                        .vias
                        .iter()
                        .any(|v| v.net == net && distance(v.at, a) < 1e-9)
                {
                    r.vias.push(Via { at: a, net });
                }
                continue;
            }
            if distance(a, bb) < 1e-9 {
                continue;
            }
            if let Some(t) = r.tracks.last_mut() {
                let cross = (t.b[0] - t.a[0]) * (bb[1] - a[1]) - (t.b[1] - t.a[1]) * (bb[0] - a[0]);
                let forward =
                    (t.b[0] - t.a[0]) * (bb[0] - a[0]) + (t.b[1] - t.a[1]) * (bb[1] - a[1]);
                if t.net == net
                    && t.layer == z
                    && distance(t.b, a) < 1e-9
                    && cross.abs() < 1e-9
                    && forward > 0.
                {
                    t.b = bb;
                    continue;
                }
            }
            r.tracks.push(Track {
                a,
                b: bb,
                layer: z,
                width: b.nets[net].width,
                net,
            });
        }
    }
}

type Masks = (Vec<bool>, Vec<bool>);
struct Congestion<'a> {
    trace: Vec<u32>,
    via: Vec<u32>,
    history: &'a [u32],
    present: u32,
}
// shortcut: 200 passes per order, use conflict-directed topology repair when more passes stop improving completion.
const NEGOTIATION_PASSES: usize = 200;

impl Grid {
    /// Connects a net's pads nearest-first into `r`; on failure removes its partial copper.
    #[allow(clippy::too_many_arguments)]
    fn connect(
        &self,
        b: &Board,
        r: &mut Routes,
        net: usize,
        reverse: bool,
        masks: Option<&Masks>,
        congestion: Option<&Congestion>,
        paths: &mut Vec<Vec<usize>>,
    ) -> bool {
        let (tracks, vias) = (r.tracks.len(), r.vias.len());
        let pads = &b.nets[net].pads;
        if pads.len() < 2 {
            return true;
        }
        let fresh;
        let masks = match masks {
            Some(m) => m,
            None => {
                fresh = self.masks(b, r, net, true);
                &fresh
            }
        };
        let mut tree = vec![];
        let mut connected = vec![pads[0]];
        let mut remaining = pads[1..].to_vec();
        while !remaining.is_empty() {
            let (a, bb) = connected
                .iter()
                .flat_map(|&a| remaining.iter().map(move |&bb| (a, bb)))
                .min_by(|&(a, bb), &(c, d)| {
                    distance(b.pads[a].at, b.pads[bb].at)
                        .total_cmp(&distance(b.pads[c].at, b.pads[d].at))
                        .then((a, bb).cmp(&(c, d)))
                })
                .unwrap();
            let (start, end) = if reverse && tree.is_empty() { (bb, a) } else { (a, bb) };
            let Some(path) = self.astar(b, r, net, start, end, &tree, masks, congestion) else {
                r.tracks.truncate(tracks);
                r.vias.truncate(vias);
                paths.clear();
                return false;
            };
            let at = if tree.is_empty() { b.pads[start].at } else { self.at(path[0]) };
            self.commit(b, r, net, at, b.pads[end].at, &path);
            tree.extend_from_slice(&path);
            paths.push(path);
            connected.push(bb);
            remaining.retain(|&p| p != bb);
        }
        true
    }

    /// Routes nets one at a time against hard obstacles, promoting failed nets for up to three passes.
    fn sequential(&self, b: &Board, mut order: Vec<usize>) -> Routes {
        let mut best = Routes {
            tracks: vec![],
            vias: vec![],
            routed: vec![false; b.nets.len()],
            passes: 0,
        };
        for pass in 1..=3 {
            let mut r = Routes {
                tracks: vec![],
                vias: vec![],
                routed: vec![false; b.nets.len()],
                passes: pass,
            };
            for &net in &order {
                r.routed[net] = self.connect(b, &mut r, net, pass > 1, None, None, &mut vec![]);
            }
            let count = r.routed.iter().filter(|&&v| v).count();
            if count > best.routed.iter().filter(|&&v| v).count() {
                best = r.clone();
            }
            if count == b.nets.len() {
                return r;
            }
            // Stable promotion preserves priority gained by difficult nets in earlier passes.
            order.sort_by_key(|&net| r.routed[net]);
        }
        best.passes = 3;
        best
    }

    /// Rip up only the nets blocking a missing connection, keeping the rest of the board intact.
    fn repair(&self, b: &Board, initial: &Routes) -> Routes {
        let mut best = initial.clone();
        let mut working = best.clone();
        let mut history = vec![0u32; self.nx * self.ny * self.layers];
        // shortcut: eight local rip-up passes, expand the blocker search when dense boards still cycle.
        for pass in 1..=8 {
            let mut changed = false;
            for net in 0..b.nets.len() {
                if working.routed[net] { continue; }
                // Fixed obstacles stay hard; remove routed copper for this tentative escape.
                let empty = Routes { tracks: vec![], vias: vec![], routed: vec![], passes: 0 };
                let masks = self.masks(b, &empty, net, false);
                let congestion = self.congestion(b, &working, net, &history, 4);
                let mut own = empty;
                let mut paths = vec![];
                if !self.connect(b, &mut own, net, pass % 2 == 0, Some(&masks), Some(&congestion), &mut paths) {
                    continue;
                }
                let mut blockers = vec![];
                for n in 0..b.nets.len() {
                    if !working.routed[n] { continue; }
                    let other = Routes {
                        tracks: working.tracks.iter().filter(|t| t.net == n).cloned().collect(),
                        vias: working.vias.iter().filter(|v| v.net == n).cloned().collect(),
                        routed: vec![], passes: 0,
                    };
                    if !clear(b, &other, &own) { blockers.push(n); }
                }
                let shared: Vec<_> = paths.iter().flat_map(|path| self.shared(&congestion, path)).collect();
                for i in shared { history[i] = history[i].saturating_add(10); }
                let mut candidate = working.clone();
                candidate.tracks.retain(|t| !blockers.contains(&t.net));
                candidate.vias.retain(|v| !blockers.contains(&v.net));
                for &n in &blockers { candidate.routed[n] = false; }
                candidate.tracks.extend(own.tracks);
                candidate.vias.extend(own.vias);
                candidate.routed[net] = true;
                candidate.passes = initial.passes + pass;
                for n in blockers {
                    let masks = self.masks(b, &candidate, n, false);
                    candidate.routed[n] = self.connect(b, &mut candidate, n, false, Some(&masks), None, &mut vec![]);
                }
                let count = candidate.routed.iter().filter(|&&v| v).count();
                if count >= working.routed.iter().filter(|&&v| v).count() {
                    working = candidate;
                    changed = true;
                    if count > best.routed.iter().filter(|&&v| v).count() { best = working.clone(); }
                    if count == b.nets.len() { return best; }
                }
            }
            if !changed { break; }
        }
        best
    }

    /// PathFinder negotiated congestion: failed and overlapping nets retry with growing copper costs.
    fn negotiate(&self, b: &Board, order: &[usize], reverse: bool) -> (Routes, bool) {
        let empty = Routes {
            tracks: vec![],
            vias: vec![],
            routed: vec![false; b.nets.len()],
            passes: NEGOTIATION_PASSES,
        };
        let cells = self.nx * self.ny * self.layers;
        // Bound cached masks to 128 MiB; larger boards compute them per net.
        let masks = if b.nets.len().saturating_mul(cells) <= 1 << 26 {
            Some((0..b.nets.len()).map(|net| self.masks(b, &empty, net, false)).collect::<Vec<_>>())
        } else {
            None
        };
        let mut nets = vec![empty.clone(); b.nets.len()];
        let mut routed = vec![false; b.nets.len()];
        let mut possible = vec![false; b.nets.len()];
        let mut best = empty.clone();
        let mut history = vec![0u32; self.nx * self.ny * self.layers];
        let mut paths = vec![vec![]; b.nets.len()];
        let mut present = 1u32;
        let mut passes = 0;
        let (mut least, mut stale) = (usize::MAX, 0);
        for pass in 1..=NEGOTIATION_PASSES {
            passes = pass;
            let mut congested = 0;
            for &net in order {
                let mut others = empty.clone();
                for n in order.iter().filter(|&&n| n != net) {
                    others.tracks.extend_from_slice(&nets[*n].tracks);
                    others.vias.extend_from_slice(&nets[*n].vias);
                }
                let congestion = self.congestion(b, &others, net, &history, present);
                let overlaps = |paths: &[Vec<usize>]| -> Vec<usize> {
                    paths.iter().flat_map(|p| self.shared(&congestion, p)).collect()
                };
                // Only nets that still overlap another net are ripped up after the first pass.
                if pass > 1 && routed[net] && overlaps(&paths[net]).is_empty() {
                    continue;
                }
                let mut own = empty.clone();
                paths[net].clear();
                let fresh;
                let mask = if let Some(masks) = &masks { &masks[net] } else {
                    fresh = self.masks(b, &empty, net, false);
                    &fresh
                };
                routed[net] = self.connect(b, &mut own, net, reverse, Some(mask), Some(&congestion), &mut paths[net]);
                possible[net] |= routed[net];
                let shared = overlaps(&paths[net]);
                congested += !shared.is_empty() as usize;
                for i in shared {
                    history[i] = history[i].saturating_add(10);
                }
                nets[net] = own;
            }
            if pass % 4 == 0 || congested == 0 {
                let candidate = self.legalize(b, &nets, &routed, order, pass);
                let count = candidate.routed.iter().filter(|&&v| v).count();
                if count > best.routed.iter().filter(|&&v| v).count() {
                    best = candidate;
                    stale = 0;
                }
                if count == possible.iter().filter(|&&v| v).count() || congested == 0 {
                    return (best, count == possible.iter().filter(|&&v| v).count());
                }
            }
            if congested < least {
                least = congested;
                stale = 0;
            } else {
                stale += 1;
            }
            // shortcut: restart after 32 stale passes, increase this when long plateaus lead to better routes.
            if stale >= 32 {
                break;
            }
            present = present.saturating_mul(2).min(1 << 12);
        }
        let candidate = self.legalize(b, &nets, &routed, order, passes);
        if candidate.routed.iter().filter(|&&v| v).count() > best.routed.iter().filter(|&&v| v).count() {
            best = candidate;
        }
        best.passes = passes;
        let complete = best.routed.iter().filter(|&&v| v).count() == possible.iter().filter(|&&v| v).count();
        (best, complete)
    }

    /// Accept only continuous, clearance-clean copper, then retry the missing nets with hard obstacles.
    fn legalize(&self, b: &Board, nets: &[Routes], routed: &[bool], order: &[usize], passes: usize) -> Routes {
        let mut kept = Routes {
            tracks: vec![], vias: vec![], routed: vec![false; b.nets.len()], passes,
        };
        for &net in order {
            if routed[net] && clear(b, &kept, &nets[net]) {
                kept.tracks.extend_from_slice(&nets[net].tracks);
                kept.vias.extend_from_slice(&nets[net].vias);
                kept.routed[net] = true;
            }
        }
        for &net in order {
            if !kept.routed[net] {
                let masks = self.masks(b, &kept, net, false);
                kept.routed[net] = self.connect(b, &mut kept, net, false, Some(&masks), None, &mut vec![]);
            }
        }
        kept
    }
}

/// True when every track and via in `add` keeps clearance to other nets' copper in `kept`.
fn clear(b: &Board, kept: &Routes, add: &Routes) -> bool {
    let gap = |n: usize, m: usize| b.clearance(n).max(b.clearance(m)) - 1e-7;
    add.tracks.iter().all(|t| {
        kept.tracks
            .iter()
            .filter(|u| u.net != t.net && u.layer == t.layer)
            .all(|u| segment_segment(t.a, t.b, u.a, u.b) - (t.width + u.width) / 2.0 >= gap(t.net, u.net))
            && kept
                .vias
                .iter()
                .filter(|v| v.net != t.net)
                .all(|v| segment_distance(v.at, t.a, t.b) - (b.via[0] + t.width) / 2.0 >= gap(t.net, v.net))
    }) && add.vias.iter().all(|v| {
        kept.tracks
            .iter()
            .filter(|u| u.net != v.net)
            .all(|u| segment_distance(v.at, u.a, u.b) - (b.via[0] + u.width) / 2.0 >= gap(v.net, u.net))
            && kept
                .vias
                .iter()
                .filter(|u| u.net != v.net)
                .all(|u| distance(u.at, v.at) - b.via[0] >= gap(v.net, u.net))
    })
}

pub fn route(b: &Board) -> Routes {
    let grid = Grid::new(b);
    let mut order: Vec<_> = (0..b.nets.len()).collect();
    let net_length = |i: usize| {
        b.nets[i]
            .pads
            .windows(2)
            .map(|p| distance(b.pads[p[0]].at, b.pads[p[1]].at))
            .sum::<f64>()
    };
    order.sort_by(|&a, &bb| net_length(a).total_cmp(&net_length(bb)).then(a.cmp(&bb)));
    let sequential = grid.sequential(b, order.clone());
    let count = |r: &Routes| r.routed.iter().filter(|&&v| v).count();
    if count(&sequential) == b.nets.len() {
        return sequential;
    }
    let mut best = grid.repair(b, &sequential);
    if count(&best) == b.nets.len() { return best; }
    // shortcut: four deterministic restarts, use local topology repair when more orders stop improving completion.
    for restart in 0..4 {
        match restart {
            1 => order.sort_by_key(|&net| best.routed[net]),
            2 => order.reverse(),
            3 => {
                let mut seed = 0x7351_b032u64;
                for i in (1..order.len()).rev() {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    order.swap(i, ((seed >> 32) as usize) % (i + 1));
                }
            }
            _ => {}
        }
        let (negotiated, complete) = grid.negotiate(b, &order, restart % 2 != 0);
        let repaired = grid.repair(b, &negotiated);
        if count(&repaired) > count(&best) {
            best = repaired;
        }
        if complete || count(&best) == b.nets.len() {
            break;
        }
    }
    best
}

pub(super) fn segment_segment(a: Point, b: Point, c: Point, d: Point) -> f64 {
    let cross = |a: Point, b: Point, c: Point| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    if cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0 {
        0.0
    } else {
        segment_distance(a, c, d)
            .min(segment_distance(b, c, d))
            .min(segment_distance(c, a, b))
            .min(segment_distance(d, a, b))
    }
}
#[cfg(test)]
fn segment_rect(a: Point, b: Point, c: Point, size: Point) -> f64 {
    let corners = [
        [c[0] - size[0] / 2.0, c[1] - size[1] / 2.0],
        [c[0] + size[0] / 2.0, c[1] - size[1] / 2.0],
        [c[0] + size[0] / 2.0, c[1] + size[1] / 2.0],
        [c[0] - size[0] / 2.0, c[1] + size[1] / 2.0],
    ];
    let mut d = rect_distance(a, c, size).min(rect_distance(b, c, size));
    for i in 0..4 {
        d = d.min(segment_segment(a, b, corners[i], corners[(i + 1) % 4]));
    }
    d
}

// Check continuous copper geometry independently from the rasterized search masks.
#[cfg(test)]
pub fn clearance_check(b: &Board, r: &Routes) -> (usize, f64) {
    let mut bad = 0;
    let mut via_pad_bad = 0;
    let mut min = f64::INFINITY;
    let mut check = |gap: f64| {
        min = min.min(gap);
        if gap + 1e-7 < b.clearance {
            bad += 1;
        }
    };
    for (i, t) in r.tracks.iter().enumerate() {
        for u in &r.tracks[..i] {
            if t.net != u.net && t.layer == u.layer {
                check(segment_segment(t.a, t.b, u.a, u.b) - (t.width + u.width) / 2.0);
            }
        }
        for p in &b.pads {
            if t.net != p.net && p.layers & (1 << t.layer) != 0 {
                let d = if p.round {
                    segment_distance(p.at, t.a, t.b) - p.size[0] / 2.0
                } else {
                    segment_rect(p.local(t.a), p.local(t.b), [0.0; 2], p.size)
                };
                check(d - t.width / 2.0);
            }
        }
        for v in &r.vias {
            if v.net != t.net {
                check(segment_distance(v.at, t.a, t.b) - (b.via[0] + t.width) / 2.0);
            }
        }
        for &(at, d) in &b.holes {
            check(segment_distance(at, t.a, t.b) - (d + t.width) / 2.0);
        }
        for p in [t.a, t.b] {
            assert!(place::inside(p, &b.outline));
            assert!(b
                .outline
                .iter()
                .zip(b.outline.iter().cycle().skip(1))
                .all(|(a, bb)| segment_distance(p, *a, *bb) + 1e-7 >= b.edge + t.width / 2.0));
        }
    }
    for (i, v) in r.vias.iter().enumerate() {
        for p in &b.pads {
            if p.drill.is_none()
                && !(b.via_in_pad && p.net != usize::MAX && p.net == v.net)
                && rect_distance(p.local(v.at), [0.; 2], p.size) <= b.via[0] / 2.0 + 1e-9
            {
                via_pad_bad += 1;
            }
            if p.net != v.net {
                let d = if p.round {
                    distance(v.at, p.at) - p.size[0] / 2.0
                } else {
                    rect_distance(p.local(v.at), [0.0; 2], p.size)
                };
                check(d - b.via[0] / 2.0);
            }
        }
        for u in &r.vias[..i] {
            if u.net != v.net {
                check(distance(u.at, v.at) - b.via[0]);
            }
        }
        for &(at, d) in &b.holes {
            check(distance(at, v.at) - (d + b.via[0]) / 2.0);
        }
    }
    (bad + via_pad_bad, min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Instant;

    #[test]
    fn changes_layers_and_keeps_rotated_pads_clear() {
        for layers in [2, 4] {
            let mut input = json!({"board":{"width":12,"height":10,"layers":layers},"parts":[
                {"ref":"R1","kind":"resistor","footprint":"0603","at":[-3,0],"rotation":35},
                {"ref":"R2","kind":"resistor","footprint":"0603","at":[3,0],"side":"bottom","rotation":-15}],
                "nets":{"N":["R1.2","R2.1"]}});
            let b =
                Board::from_prepared(&super::super::prepare(&input.to_string()).unwrap()).unwrap();
            let r = route(&b);
            assert_eq!(r.routed, vec![true]);
            assert_eq!(r.vias.len(), 1);
            assert_eq!(clearance_check(&b, &r).0, 0);
            assert_eq!(r, route(&b));
            input["keepouts"] = json!([{"rect":[0,0,1,10],"layers":if layers==2 {vec!["top","bottom"]} else {vec!["top","bottom","inner1","inner2"]}}]);
            let b =
                Board::from_prepared(&super::super::prepare(&input.to_string()).unwrap()).unwrap();
            let r = route(&b);
            assert_eq!(r.routed, vec![false]);
            assert_eq!(r.passes, 3);
            assert!(r.tracks.is_empty());
        }
    }

    #[test]
    fn reversing_at_a_branch_keeps_the_previous_pad_connected() {
        let mut b = Board::from_prepared(
            &super::super::prepare(r#"{"board":{"width":10,"height":10}}"#).unwrap(),
        )
        .unwrap();
        b.nets.push(Net {
            name: "N".into(),
            pads: vec![0, 1, 2],
            width: 0.2,
            clearance: 0.2,
        });
        for (i, at) in [[0., 0.], [2., 0.], [1., 0.]].into_iter().enumerate() {
            b.pads.push(Pad {
                name: format!("P{i}.1"),
                at,
                size: [0.3; 2],
                rotation: 0.,
                round: false,
                layers: 1,
                drill: None,
                plated: true,
                net: 0,
            });
        }
        let grid = Grid::new(&b);
        let mut r = Routes {
            tracks: vec![],
            vias: vec![],
            routed: vec![true],
            passes: 1,
        };
        grid.commit(
            &b,
            &mut r,
            0,
            b.pads[0].at,
            b.pads[1].at,
            &[grid.node(b.pads[0].at, 0), grid.node(b.pads[1].at, 0)],
        );
        grid.commit(
            &b,
            &mut r,
            0,
            b.pads[1].at,
            b.pads[2].at,
            &[grid.node(b.pads[1].at, 0), grid.node(b.pads[2].at, 0)],
        );
        assert_eq!(r.tracks.len(), 2);
        assert_eq!(r.tracks[0].b, b.pads[1].at);
        assert_eq!(r.tracks[1].a, b.pads[1].at);
        assert_eq!(r.tracks[1].b, b.pads[2].at);
    }

    #[test]
    fn huge_board_fails_before_allocating_a_grid() {
        let prepared =
            super::super::prepare(r#"{"board":{"width":1e100,"height":1e100}}"#).unwrap();
        assert_eq!(
            Board::from_prepared(&prepared).unwrap_err(),
            "board is too large for the routing grid"
        );
    }

    #[test]
    fn inner_layer_and_concave_outline_obstacles() {
        let input = json!({"board":{"outline":[[-6,-5],[6,-5],[6,5],[1,5],[1,0],[-1,0],[-1,5],[-6,5]],"layers":4},"parts":[
            {"ref":"R1","kind":"resistor","footprint":"0402","at":[-3,2]},
            {"ref":"R2","kind":"resistor","footprint":"0402","at":[3,2]}],
            "keepouts":[{"rect":[0,-2,2,6],"layers":["top","bottom","inner2"]}],"nets":{"N":["R1.1","R2.1"]}});
        let b = Board::from_prepared(&super::super::prepare(&input.to_string()).unwrap()).unwrap();
        let r = route(&b);
        assert_eq!(r.routed, vec![true]);
        assert!(r.tracks.iter().any(|t| t.layer == 2));
        assert_eq!(clearance_check(&b, &r).0, 0);
        assert_eq!(r, route(&b));
    }

    /// Rows of six SOIC8/QFN32/0603 parts with `nets` random two-pin nets, 8 x 7.5 mm pitch.
    fn stress_board(parts: usize, nets: usize, seed: u64) -> Board {
        let height = parts.div_ceil(6) as f64 * 7.5 + 2.5;
        let mut b = Board::from_prepared(
            &super::super::prepare(&json!({"board":{"width":50,"height":height}}).to_string())
                .unwrap(),
        )
        .unwrap();
        for i in 0..parts {
            let fp = match i % 3 {
                0 => "soic8",
                1 => "qfn32",
                _ => "0603",
            };
            let lands = super::super::footprint(fp).unwrap();
            let at = [-20. + (i % 6) as f64 * 8., 5. - height / 2. + (i / 6) as f64 * 7.5];
            let bottom = i % 5 == 0;
            for p in lands.pads {
                b.pads.push(Pad {
                    name: format!("P{}.{}", i + 1, p.number),
                    at: [
                        at[0] + if bottom { -p.at[0] } else { p.at[0] },
                        at[1] - p.at[1],
                    ],
                    size: p.size,
                    rotation: 0.,
                    round: false,
                    layers: if bottom { 2 } else { 1 },
                    drill: None,
                    plated: true,
                    net: usize::MAX,
                });
            }
        }
        let mut pins: Vec<_> = (0..b.pads.len()).collect();
        let mut seed = seed;
        for i in (1..pins.len()).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            pins.swap(i, ((seed >> 32) as usize) % (i + 1));
        }
        for i in 0..nets {
            let pads = vec![pins[i * 2], pins[i * 2 + 1]];
            for &p in &pads {
                b.pads[p].net = i;
            }
            b.nets.push(Net {
                name: format!("N{i:02}"),
                pads,
                width: 0.2,
                clearance: 0.2,
            });
        }
        b
    }

    #[test]
    fn negotiation_routes_nets_that_sequential_routing_blocks() {
        let b = stress_board(6, 12, 0x7351_b032 + 6);
        let grid = Grid::new(&b);
        let order: Vec<_> = (0..b.nets.len()).collect();
        let sequential = grid.sequential(&b, order);
        assert_eq!(sequential.routed.iter().filter(|v| **v).count(), 9);
        let r = route(&b);
        assert_eq!(r.routed, vec![true; 12]);
        assert_eq!(r.passes, 9);
        assert_eq!(clearance_check(&b, &r).0, 0);
        assert_eq!(r, route(&b));
    }

    #[test]
    #[ignore = "routing benchmark; run explicitly in release"]
    fn stress_and_555_benchmarks() {
        let b = stress_board(30, 40, 0x7351_b032);
        let mut filled = b.clone();
        filled.via_in_pad = true;
        let mut four = b.clone();
        four.layers = 4;
        let mut four_filled = four.clone();
        four_filled.via_in_pad = true;
        let blinker = Board::from_prepared(&super::super::prepare(BLINKER).unwrap()).unwrap();
        for (name, b, minimum) in [
            ("30-part stress 2L, open vias", &b, 35),
            ("30-part stress 2L, filled vias", &filled, 40),
            ("30-part stress 4L, open vias", &four, 35),
            ("30-part stress 4L, filled vias", &four_filled, 40),
            ("555 blinker", &blinker, blinker.nets.len()),
        ] {
            let start = Instant::now();
            let r = route(b);
            let elapsed = start.elapsed();
            let count = r.routed.iter().filter(|v| **v).count();
            eprintln!(
                "{name}: {count}/{} nets, {} vias, {} passes, routing {:.3} ms",
                b.nets.len(),
                r.vias.len(),
                r.passes,
                elapsed.as_secs_f64() * 1000.
            );
            assert!(count >= minimum);
            assert_eq!(clearance_check(b, &r).0, 0);
            assert_eq!(r, route(b));
        }
    }
    #[test]
    fn exposed_pad_escape_requires_filled_vias() {
        let mut input = json!({"board":{"width":16,"height":12},"parts":[
            {"ref":"U1","kind":"chip","footprint":"qfn32","at":[0,0]},
            {"ref":"R1","kind":"resistor","footprint":"0603","at":[5,0]}],
            "nets":{"GND":["U1.33","R1.1"]}});
        let b = Board::from_prepared(&super::super::prepare(&input.to_string()).unwrap()).unwrap();
        assert_eq!(route(&b).routed, vec![false]);
        input["rules"] = json!({"via_in_pad": true});
        let b = Board::from_prepared(&super::super::prepare(&input.to_string()).unwrap()).unwrap();
        let r = route(&b);
        assert_eq!(r.routed, vec![true]);
        assert!(r.vias.iter().any(|v| rect_distance(v.at, [0.; 2], [3.1; 2]) <= b.via[0] / 2.));
        assert_eq!(clearance_check(&b, &r).0, 0);
        assert_eq!(r, route(&b));
        let mut forbidden = b.clone();
        forbidden.via_in_pad = false;
        assert!(clearance_check(&forbidden, &r).0 > 0);
    }

    #[test]
    fn multi_pin_tree_escapes_crossing_nets() {
        let mut b = stress_board(6, 18, 0x7351_b032 + 26);
        let pads: Vec<_> = b.nets.drain(..3).flat_map(|n| n.pads).collect();
        b.nets.insert(0, Net { name: "POWER".into(), pads, width: 0.2, clearance: 0.2 });
        for (n, net) in b.nets.iter().enumerate() {
            for &p in &net.pads { b.pads[p].net = n; }
        }
        assert_eq!(b.nets[0].pads.len(), 6);
        let r = route(&b);
        assert_eq!(r.routed, vec![true; 16]);
        assert_eq!(clearance_check(&b, &r).0, 0);
        assert_eq!(r, route(&b));
    }

    const BLINKER: &str = r#"{
  "board": {"width": 40, "height": 30, "layers": 2, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "parts": [
    {"ref": "U1", "kind": "chip", "footprint": "soic8", "at": [0, 0], "pins": {"1": "GND", "2": "TRIG", "3": "OUT", "4": "RESET", "5": "CTRL", "6": "THRES", "7": "DISCH", "8": "VCC"}},
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0603", "at": [-10, 5]},
    {"ref": "R2", "kind": "resistor", "value": "10k", "footprint": "0603", "at": [-10, -5]},
    {"ref": "R3", "kind": "resistor", "value": "330", "footprint": "0603", "at": [10, 5]},
    {"ref": "C1", "kind": "capacitor", "value": "10uF", "footprint": "0603", "at": [-10, -10]},
    {"ref": "C2", "kind": "capacitor", "value": "10nF", "footprint": "0603", "at": [10, -5]},
    {"ref": "LED1", "kind": "led", "value": "red", "footprint": "0603", "at": [15, 5]},
    {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [0, -10]}
  ],
  "nets": {
    "VCC": ["J1.1", "U1.8", "U1.4", "R1.1"],
    "GND": ["J1.2", "U1.1", "C1.2", "C2.2", "LED1.K"],
    "DISCH": ["R1.2", "R2.1", "U1.7"],
    "TIMING": ["R2.2", "C1.1", "U1.6", "U1.2"],
    "CTRL": ["U1.5", "C2.1"],
    "OUT": ["U1.3", "R3.1"],
    "LED_NET": ["R3.2", "LED1.A"]
  }
}"#;
}
