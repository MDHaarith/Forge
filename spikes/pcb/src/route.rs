use crate::footprint;
use std::{cmp::Reverse, collections::BinaryHeap, time::Instant};

pub type Point = [f64; 2];
#[derive(Clone, Debug)]
pub struct Pad {
    pub name: String,
    pub at: Point,
    pub size: Point,
    pub round: bool,
    pub layers: u8,
    pub drill: f64,
    pub net: usize,
}
#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub at: Point,
    pub body: [f64; 3],
    pub bottom: bool,
}
#[derive(Clone, Debug)]
pub struct Net {
    pub name: String,
    pub pads: Vec<usize>,
    pub width: f64,
}
#[derive(Clone, Debug)]
pub struct Board {
    pub size: Point,
    pub pads: Vec<Pad>,
    pub parts: Vec<Part>,
    pub nets: Vec<Net>,
    pub holes: Vec<(Point, f64)>,
    pub keepouts: Vec<(Point, Point, u8)>,
    pub clearance: f64,
    pub edge: f64,
    pub via: [f64; 2],
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

impl Board {
    fn empty(size: Point) -> Self {
        Self {
            size,
            pads: vec![],
            parts: vec![],
            nets: vec![],
            holes: vec![],
            keepouts: vec![],
            clearance: 0.2,
            edge: 0.3,
            via: [0.6, 0.3],
        }
    }
    fn part(&mut self, name: &str, fp: &str, at: Point, rotation: f64, bottom: bool) {
        let angle = rotation.to_radians();
        let transform = |p: Point| {
            [
                at[0] + p[0] * angle.cos() - p[1] * angle.sin(),
                at[1] + p[0] * angle.sin() + p[1] * angle.cos(),
            ]
        };
        let body = match fp {
            "0402" => [1.0, 0.5, 0.4],
            "0603" => [1.6, 0.8, 0.6],
            "soic8" => [3.9, 4.9, 1.75],
            "qfn32" => [5.0, 5.0, 1.0],
            "pinheader_1x2" => [2.54, 5.08, 8.5],
            _ => unreachable!(),
        };
        let lands = if fp == "pinheader_1x2" {
            (0..2)
                .map(|i| footprint::Land {
                    pin: i + 1,
                    at: [0.0, (i as f64 - 0.5) * 2.54],
                    size: [1.7, 1.7],
                })
                .collect()
        } else {
            footprint::pattern(fp)
        };
        for p in lands {
            let size = if rotation == 90.0 {
                [p.size[1], p.size[0]]
            } else {
                p.size
            };
            self.pads.push(Pad {
                name: format!("{name}.{}", p.pin),
                at: transform([if bottom { -p.at[0] } else { p.at[0] }, -p.at[1]]),
                size,
                round: fp == "pinheader_1x2",
                layers: if fp == "pinheader_1x2" {
                    3
                } else if bottom {
                    2
                } else {
                    1
                },
                drill: if fp == "pinheader_1x2" { 1.0 } else { 0.0 },
                net: usize::MAX,
            });
        }
        self.parts.push(Part {
            name: name.into(),
            at,
            body: if rotation == 90.0 {
                [body[1], body[0], body[2]]
            } else {
                body
            },
            bottom,
        });
    }
    fn net(&mut self, name: &str, pins: &[&str], width: f64) {
        let pads: Vec<_> = pins
            .iter()
            .map(|pin| self.pads.iter().position(|p| p.name == *pin).unwrap())
            .collect();
        let id = self.nets.len();
        for &i in &pads {
            assert_eq!(self.pads[i].net, usize::MAX);
            self.pads[i].net = id;
        }
        self.nets.push(Net {
            name: name.into(),
            pads,
            width,
        });
    }
}

pub fn led() -> Board {
    let mut b = Board::empty([30.0, 20.0]);
    b.part("R1", "0402", [-6.0, 0.0], 0.0, false);
    b.part("LED1", "0603", [6.0, 0.0], 90.0, false);
    b.part("J1", "pinheader_1x2", [0.0, -6.0], 0.0, false);
    // A fixed, free placement for the plan's U1, which has no 'at'.
    b.part("U1", "soic8", [0.0, 4.0], 0.0, true);
    b.net("IN", &["J1.1", "R1.1"], 0.2);
    b.net("A", &["R1.2", "LED1.1"], 0.2);
    b.net("GND", &["LED1.2", "J1.2", "U1.8"], 0.4);
    b.net("VCC", &["U1.1"], 0.4);
    b.holes.push(([12.0, 7.0], 3.2));
    b.keepouts.push(([-3.0, 16.0], [32.0, 8.0], 2));
    b
}

pub fn generated() -> Board {
    let mut b = Board::empty([50.0, 40.0]);
    for i in 0..30 {
        let fp = match i % 3 {
            0 => "soic8",
            1 => "qfn32",
            _ => "0603",
        };
        b.part(
            &format!("P{}", i + 1),
            fp,
            [-20.0 + (i % 6) as f64 * 8.0, -15.0 + (i / 6) as f64 * 7.5],
            0.0,
            i % 5 == 0,
        );
    }
    // Fixed-seed Fisher-Yates: every net gets two unique terminals, spread across the whole board.
    let mut pins: Vec<_> = b.pads.iter().map(|p| p.name.clone()).collect();
    let mut seed = 0x7351_b032_u64;
    for i in (1..pins.len()).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        pins.swap(i, ((seed >> 32) as usize) % (i + 1));
    }
    for i in 0..40 {
        b.net(&format!("N{i:02}"), &[&pins[i * 2], &pins[i * 2 + 1]], 0.2);
    }
    b
}

struct Grid {
    nx: usize,
    ny: usize,
    g: f64,
    size: Point,
}
impl Grid {
    fn new(b: &Board) -> Self {
        let g = (b.clearance / 2.0).max(0.05);
        Self {
            nx: (b.size[0] / g).floor() as usize + 1,
            ny: (b.size[1] / g).floor() as usize + 1,
            g,
            size: b.size,
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
            x as f64 * self.g - self.size[0] / 2.0,
            y as f64 * self.g - self.size[1] / 2.0,
        ]
    }
    fn node(&self, p: Point, z: usize) -> usize {
        self.id(
            ((p[0] + self.size[0] / 2.0) / self.g).round() as usize,
            ((p[1] + self.size[1] / 2.0) / self.g).round() as usize,
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
        let low = |v: f64, axis: usize| {
            (((v + self.size[axis] / 2.0) / self.g).floor() as isize).max(0) as usize
        };
        let high = |v: f64, axis: usize| {
            (((v + self.size[axis] / 2.0) / self.g).ceil() as isize).max(0) as usize
        };
        let x0 = low(c[0] - size[0] / 2.0, 0).min(self.nx);
        let y0 = low(c[1] - size[1] / 2.0, 1).min(self.ny);
        let x1 = high(c[0] + size[0] / 2.0, 0).min(self.nx - 1);
        let y1 = high(c[1] + size[1] / 2.0, 1).min(self.ny - 1);
        for z in 0..2 {
            if layers & (1 << z) == 0 {
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let i = self.id(x, y, z);
                    if within(self.at(i)) {
                        blocked[i] = true;
                    }
                }
            }
        }
    }
    fn obstacles(&self, b: &Board, r: &Routes, net: usize, radius: f64) -> Vec<bool> {
        let mut blocked = vec![false; self.nx * self.ny * 2];
        // Half-cell guard covers the complete orthogonal segment, not only each sample point.
        let guard = self.g / 2.0;
        let margin = b.edge + radius + guard;
        for (i, v) in blocked.iter_mut().enumerate() {
            let p = self.at(i);
            *v = p[0].abs() > b.size[0] / 2.0 - margin || p[1].abs() > b.size[1] / 2.0 - margin;
        }
        let c = b.clearance + radius + guard;
        for p in &b.pads {
            if p.net == net {
                continue;
            }
            // Square expansion is conservative along an entire orthogonal edge, without an extra half-cell pad penalty.
            let pad_c = if p.round { c } else { b.clearance + radius };
            let ext = [p.size[0] + 2.0 * pad_c, p.size[1] + 2.0 * pad_c];
            self.mark(&mut blocked, p.at, ext, p.layers, |q| {
                if p.round {
                    distance(q, p.at) < p.size[0] / 2.0 + pad_c
                } else {
                    (q[0] - p.at[0]).abs() < ext[0] / 2.0 && (q[1] - p.at[1]).abs() < ext[1] / 2.0
                }
            });
        }
        for &(at, d) in &b.holes {
            let radius = d / 2.0 + c;
            self.mark(&mut blocked, at, [radius * 2.0; 2], 3, |q| {
                distance(q, at) < radius
            });
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
        for t in &r.tracks {
            if t.net == net {
                continue;
            }
            let radius = t.width / 2.0 + c;
            let center = [(t.a[0] + t.b[0]) / 2.0, (t.a[1] + t.b[1]) / 2.0];
            let size = [
                (t.a[0] - t.b[0]).abs() + 2.0 * radius,
                (t.a[1] - t.b[1]).abs() + 2.0 * radius,
            ];
            self.mark(&mut blocked, center, size, 1 << t.layer, |q| {
                segment_distance(q, t.a, t.b) < radius
            });
        }
        for v in &r.vias {
            if v.net == net {
                continue;
            }
            let radius = b.via[0] / 2.0 + c;
            self.mark(&mut blocked, v.at, [radius * 2.0; 2], 3, |q| {
                distance(q, v.at) < radius
            });
        }
        blocked
    }

    // shortcut: grid maze router, upgrade to a shape-based router when boards over ~60 parts fail to route.
    fn astar(&self, b: &Board, r: &Routes, net: usize, a: usize, bb: usize) -> Option<Vec<usize>> {
        let blocked = self.obstacles(b, r, net, b.nets[net].width / 2.0);
        let via_blocked = self.obstacles(b, r, net, b.via[0] / 2.0);
        let source = &b.pads[a];
        let target = &b.pads[bb];
        let target_xy = self.xyz(self.node(target.at, 0));
        let heuristic = |i| {
            let (x, y, _) = self.xyz(i);
            ((x.abs_diff(target_xy.0) + y.abs_diff(target_xy.1)) * 10) as u32
        };
        let mut cost = vec![u32::MAX; blocked.len()];
        let mut parent = vec![usize::MAX; blocked.len()];
        let mut queue = BinaryHeap::new();
        for z in 0..2 {
            if source.layers & (1 << z) != 0 {
                let i = self.node(source.at, z);
                if !blocked[i] {
                    cost[i] = 0;
                    queue.push(Reverse((heuristic(i), 0, i)));
                }
            }
        }
        while let Some(Reverse((_, g, i))) = queue.pop() {
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
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (0, 0)] {
                let xx = x as isize + dx;
                let yy = y as isize + dy;
                if xx < 0 || yy < 0 || xx >= self.nx as isize || yy >= self.ny as isize {
                    continue;
                }
                let is_via = dx == 0 && dy == 0;
                let nz = if is_via { 1 - z } else { z };
                let next = self.id(xx as usize, yy as usize, nz);
                if blocked[next] || (is_via && (via_blocked[i] || via_blocked[next])) {
                    continue;
                }
                // Existing same-net PTH pads connect layers without inserting a second drill.
                let through = is_via
                    && b.pads.iter().any(|p| {
                        p.layers == 3 && p.net == net && distance(self.at(i), p.at) < p.drill / 2.0
                    });
                let nc = g + if is_via {
                    if through {
                        0
                    } else {
                        300
                    }
                } else {
                    10
                };
                if nc < cost[next] {
                    cost[next] = nc;
                    parent[next] = i;
                    queue.push(Reverse((nc + heuristic(next), nc, next)));
                }
            }
        }
        None
    }
    fn commit(&self, b: &Board, r: &mut Routes, net: usize, a: usize, bb: usize, path: &[usize]) {
        let mut points = vec![(b.pads[a].at, self.xyz(path[0]).2)];
        points.extend(path.iter().map(|&i| (self.at(i), self.xyz(i).2)));
        points.push((b.pads[bb].at, self.xyz(*path.last().unwrap()).2));
        for pair in points.windows(2) {
            let ((a, z), (bb, zz)) = (pair[0], pair[1]);
            if z != zz {
                let through = b
                    .pads
                    .iter()
                    .any(|p| p.layers == 3 && p.net == net && distance(a, p.at) < p.drill / 2.0);
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
                if t.net == net && t.layer == z && distance(t.b, a) < 1e-9 && cross.abs() < 1e-9 {
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
            let before_tracks = r.tracks.len();
            let before_vias = r.vias.len();
            let pads = &b.nets[net].pads;
            let mut connected = vec![pads[0]];
            let mut remaining = pads[1..].to_vec();
            let mut ok = true;
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
                if let Some(path) = grid.astar(b, &r, net, a, bb) {
                    grid.commit(b, &mut r, net, a, bb, &path);
                    connected.push(bb);
                    remaining.retain(|&p| p != bb);
                } else {
                    ok = false;
                    break;
                }
            }
            if !ok {
                r.tracks.truncate(before_tracks);
                r.vias.truncate(before_vias);
            }
            r.routed[net] = ok;
        }
        let count = r.routed.iter().filter(|&&v| v).count();
        if count > best.routed.iter().filter(|&&v| v).count() {
            best = r.clone();
        }
        if count == b.nets.len() {
            return r;
        }
        // Rip up all tracks and retry, moving failures ahead while retaining short-first ordering within groups.
        order.sort_by(|&a, &bb| {
            r.routed[a]
                .cmp(&r.routed[bb])
                .then(net_length(a).total_cmp(&net_length(bb)))
                .then(a.cmp(&bb))
        });
    }
    best.passes = 3;
    best
}

fn segment_segment(a: Point, b: Point, c: Point, d: Point) -> f64 {
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
pub fn clearance_check(b: &Board, r: &Routes) -> (usize, f64) {
    let mut bad = 0;
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
                    segment_rect(t.a, t.b, p.at, p.size)
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
            assert!(p[0].abs() + t.width / 2.0 <= b.size[0] / 2.0 - b.edge + 1e-7);
            assert!(p[1].abs() + t.width / 2.0 <= b.size[1] / 2.0 - b.edge + 1e-7);
        }
    }
    for (i, v) in r.vias.iter().enumerate() {
        for p in &b.pads {
            if p.net != v.net {
                let d = if p.round {
                    distance(v.at, p.at) - p.size[0] / 2.0
                } else {
                    rect_distance(v.at, p.at, p.size)
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
    (bad, min)
}

pub fn benchmark(name: &str, b: &Board, log: &mut String) -> Routes {
    let t = Instant::now();
    let r = route(b);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let connected = r.routed.iter().filter(|&&v| v).count();
    let wired = r
        .routed
        .iter()
        .enumerate()
        .filter(|&(i, &v)| v && b.nets[i].pads.len() > 1)
        .count();
    let links: usize = b
        .nets
        .iter()
        .enumerate()
        .filter(|(i, _)| r.routed[*i])
        .map(|(_, n)| n.pads.len() - 1)
        .sum();
    let (bad, min) = clearance_check(b, &r);
    crate::report(log,format!("router {name}: board={} x {} mm layers=2 parts={} pads={} nets={} routed={connected} unrouted={} routed_multiterminal_nets={wired} connection_links={links} vias={} tracks={} grid={:.3} mm passes={} time_ms={ms:.3} clearance_violations={bad} min_gap={min:.6} mm",b.size[0],b.size[1],b.parts.len(),b.pads.len(),b.nets.len(),b.nets.len()-connected,r.vias.len(),r.tracks.len(),(b.clearance/2.0).max(0.05),r.passes));
    assert_eq!(
        bad, 0,
        "router produced a physical short or clearance violation"
    );
    let repeat = route(b);
    assert_eq!(r, repeat, "router must be deterministic");
    crate::report(
        log,
        format!(
            "router {name}: identical second run=true; unrouted={:?}",
            b.nets
                .iter()
                .enumerate()
                .filter(|(i, _)| !r.routed[*i])
                .map(|(_, n)| n.name.as_str())
                .collect::<Vec<_>>()
        ),
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_layers_and_reports_a_blocked_board() {
        let mut b = Board::empty([8.0, 6.0]);
        b.part("R1", "0603", [-2.0, 0.0], 0.0, false);
        b.part("R2", "0603", [2.0, 0.0], 0.0, true);
        assert!(
            b.pads.iter().find(|p| p.name == "R2.1").unwrap().at[0] > 2.0,
            "bottom placement must mirror pin numbering"
        );
        b.net("N", &["R1.2", "R2.1"], 0.2);
        let r = route(&b);
        assert_eq!(r.routed, vec![true]);
        assert_eq!(r.vias.len(), 1);
        assert_eq!(clearance_check(&b, &r).0, 0);
        b.keepouts.push(([0.0, 0.0], [1.0, 6.0], 3));
        let r = route(&b);
        assert_eq!(r.routed, vec![false]);
        assert_eq!(r.passes, 3);
        assert!(r.tracks.is_empty());
    }
}
