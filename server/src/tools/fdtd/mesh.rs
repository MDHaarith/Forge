use super::{
    geometry::{Board, Point},
    C0,
};

pub const PML: usize = 8;
#[derive(Clone)]
pub struct Mesh {
    pub axes: [Vec<f64>; 3],
    pub dt: f64,
    pub periodic_y: bool,
}
impl Mesh {
    pub fn new(axes: [Vec<f64>; 3]) -> Result<Self, String> {
        if axes.iter().any(|a| {
            a.len() < 3
                || a.iter().any(|x| !x.is_finite())
                || a.windows(2).any(|w| w[1] - w[0] < 1e-10)
        }) {
            return Err("invalid FDTD mesh".into());
        }
        let cells = axes
            .iter()
            .try_fold(1usize, |n, a| n.checked_mul(a.len()))
            .ok_or("FDTD mesh exceeds the cell limit")?;
        // shortcut: cap meshes at 4M nodes to bound field memory, use tiled storage for larger boards.
        if cells > 4_000_000 {
            return Err(format!("FDTD mesh has {cells} cells; limit is 4000000"));
        }
        let inv_sum: f64 = axes
            .iter()
            .map(|a| {
                a.windows(2)
                    .map(|w| w[1] - w[0])
                    .fold(f64::INFINITY, f64::min)
                    .powi(-2)
            })
            .sum();
        Ok(Self {
            axes,
            dt: 0.95 / (C0 * inv_sum.sqrt()),
            periodic_y: false,
        })
    }
    pub fn dims(&self) -> [usize; 3] {
        self.axes.each_ref().map(|a| a.len())
    }
    pub fn cells(&self) -> usize {
        self.dims().iter().product()
    }
    pub fn nearest(&self, axis: usize, v: f64) -> usize {
        self.axes[axis]
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (*a - v).abs().total_cmp(&(*b - v).abs()))
            .unwrap()
            .0
    }
    pub fn step(&self, axis: usize, i: usize) -> f64 {
        let a = &self.axes[axis];
        a[(i + 1).min(a.len() - 1)] - a[i.min(a.len() - 2)]
    }
    pub fn dual(&self, axis: usize, i: usize) -> f64 {
        let a = &self.axes[axis];
        if self.periodic_y && axis == 1 {
            return self.step(axis, i);
        }
        if i == 0 {
            (a[1] - a[0]) / 2.
        } else if i == a.len() - 1 {
            (a[i] - a[i - 1]) / 2.
        } else {
            (a[i + 1] - a[i - 1]) / 2.
        }
    }
}
fn merge(mut lines: Vec<f64>, fixed: &[f64], tol: f64) -> Vec<f64> {
    lines.extend(fixed);
    lines.iter_mut().for_each(|x| *x = (*x * 1e6).round() / 1e6);
    lines.sort_by(f64::total_cmp);
    lines.dedup();
    let fixed: Vec<_> = fixed.iter().map(|x| (*x * 1e6).round() / 1e6).collect();
    let mut out = Vec::new();
    for v in lines {
        if let Some(last) = out.last_mut() {
            if v - *last < tol && !(fixed.contains(&v) && fixed.contains(last)) {
                if fixed.contains(&v) {
                    *last = v;
                }
                continue;
            }
        }
        out.push(v);
    }
    out
}
pub(super) fn smooth(lines: Vec<f64>, limits: &[f64]) -> Result<Vec<f64>, String> {
    // shortcut: bound grading to 4096 lines per axis and 256 passes, use a sparse/tiled mesh for larger geometries.
    let gaps: Vec<_> = lines.windows(2).map(|w| w[1] - w[0]).collect();
    let mut caps: Vec<_> = limits.iter().map(|&max| [max; 2]).collect();
    for _ in 0..256 {
        let mut widths = Vec::new();
        let mut total = 1;
        for (i, &length) in gaps.iter().enumerate() {
            let max = limits[i];
            let mut count = (length / max).ceil().max(1.) as usize;
            loop {
                if count > 4096 {
                    return Err("FDTD mesh grading exceeds 4096 lines per interval".into());
                }
                let mut sizes: Vec<_> = (0..count)
                    .map(|k| {
                        max.min(caps[i][0] * 1.4_f64.powi(k as i32))
                            .min(caps[i][1] * 1.4_f64.powi((count - 1 - k) as i32))
                    })
                    .collect();
                let sum: f64 = sizes.iter().sum();
                if sum >= length * (1. - 1e-12) {
                    for x in &mut sizes {
                        *x *= length / sum;
                    }
                    total += sizes.len();
                    if total > 4096 {
                        return Err("FDTD mesh grading exceeds 4096 lines per axis".into());
                    }
                    widths.push(sizes);
                    break;
                }
                count += 1;
            }
        }
        let mut changed = false;
        for i in 1..widths.len() {
            let left = *widths[i - 1].last().unwrap();
            let right = widths[i][0];
            if right > left * 1.4 * (1. + 1e-10) {
                caps[i][0] = caps[i][0].min(left * 1.4);
                changed = true;
            }
            if left > right * 1.4 * (1. + 1e-10) {
                caps[i - 1][1] = caps[i - 1][1].min(right * 1.4);
                changed = true;
            }
        }
        if !changed {
            let mut out = vec![lines[0]];
            for (i, sizes) in widths.iter().enumerate() {
                let mut x = lines[i];
                for size in sizes.iter().take(sizes.len() - 1) {
                    x += size;
                    out.push(x);
                }
                out.push(lines[i + 1]);
            }
            return Ok(out);
        }
    }
    Err("FDTD mesh grading did not converge".into())
}
pub fn build(board: &Board, fmin: f64, fmax: f64, er: f64, accurate: bool) -> Result<Mesh, String> {
    let res_air = C0 / fmax * 1e3 / if accurate { 30. } else { 20. };
    let res_sub = res_air / er.sqrt();
    let edge = (if accurate { 0.3_f64 } else { 0.5_f64 }).min(res_sub / 2.);
    let tol = if accurate { 0.05 } else { 0.1 };
    let mut lines: [Vec<f64>; 3] = std::array::from_fn(|_| vec![]);
    let bounds = |pts: Vec<Point>| {
        [
            pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
            pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max),
            pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
            pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max),
        ]
    };
    let ant = bounds(
        board
            .copper
            .iter()
            .flat_map(|s| s.outer.iter().copied())
            .collect(),
    );
    let metal = |z, p| {
        board
            .copper
            .iter()
            .chain(&board.pours)
            .any(|s| s.z == z && s.contains(p))
    };
    for (pour, s) in board
        .copper
        .iter()
        .map(|s| (false, s))
        .chain(board.pours.iter().map(|s| (true, s)))
    {
        for poly in std::iter::once(&s.outer).chain(s.holes.iter()) {
            for (a, b) in poly
                .iter()
                .zip(poly.iter().cycle().skip(1))
                .take(poly.len())
            {
                for axis in 0..2 {
                    let other = 1 - axis;
                    if (a[axis] - b[axis]).abs() > 1e-9 || (a[other] - b[other]).abs() < 1e-9 {
                        continue;
                    }
                    let mut p = [(a[0] + b[0]) / 2., (a[1] + b[1]) / 2.];
                    p[axis] -= 1e-4;
                    let low = metal(s.z, p);
                    p[axis] += 2e-4;
                    let high = metal(s.z, p);
                    if low == high {
                        continue;
                    }
                    let sign = if high { 1. } else { -1. };
                    for v in [a[axis] + sign * edge / 3., a[axis] - sign * 2. * edge / 3.] {
                        if !pour || (v >= ant[axis * 2] - 5. && v <= ant[axis * 2 + 1] + 5.) {
                            lines[axis].push(v);
                        }
                    }
                }
            }
        }
    }
    let b = bounds(board.outline.clone());
    let margin = C0 / fmin * 1e3 / 4.;
    for axis in 0..2 {
        for (a, c) in board
            .outline
            .iter()
            .zip(board.outline.iter().cycle().skip(1))
            .take(board.outline.len())
        {
            if (a[axis] - c[axis]).abs() < 1e-9 {
                lines[axis].push(a[axis]);
            }
        }
        lines[axis].extend([
            b[2 * axis],
            b[2 * axis + 1],
            b[2 * axis] - margin,
            b[2 * axis + 1] + margin,
        ]);
    }
    lines[2].extend(&board.layers);
    let count = if accurate { 6 } else { 4 };
    lines[2].extend((0..=count).map(|i| board.thickness * i as f64 / count as f64));
    lines[2].extend([-margin, board.thickness + margin]);
    for (axis, line) in lines.iter_mut().enumerate() {
        let mut fixed = vec![board.feed.start[axis], board.feed.stop[axis]];
        if axis < 2 {
            fixed.extend(board.barrels.iter().map(|b| b.xy[axis]));
        } else {
            fixed.extend(&board.layers);
        }
        let merged = merge(
            std::mem::take(line),
            &fixed,
            if axis == 2 { tol / 2. } else { tol },
        );
        let limits: Vec<_> = merged
            .windows(2)
            .map(|w| {
                if axis == 2 || w[1] <= b[axis * 2] || w[0] >= b[axis * 2 + 1] {
                    res_air
                } else {
                    res_sub
                }
            })
            .collect();
        *line = smooth(merged, &limits)?;
        // The wavelength/4 air box remains outside the Huygens surface; append the CPML beyond it.
        let left = line[1] - line[0];
        let right = line[line.len() - 1] - line[line.len() - 2];
        let mut padded: Vec<_> = (1..=PML).rev().map(|i| line[0] - left * i as f64).collect();
        padded.extend(line.iter());
        padded.extend((1..=PML).map(|i| line[line.len() - 1] + right * i as f64));
        *line = padded.into_iter().map(|v| v * 1e-3).collect();
    }
    Mesh::new(lines)
}
