//! Huygens equivalence currents from spatially and temporally collocated Yee fields.
use super::{mesh::Mesh, Complex, C0, ETA0};
use std::f64::consts::{PI, TAU};

#[derive(Clone)]
pub struct Surface {
    pub xyz: [usize; 3],
    pub axis: usize,
    pub sign: i8,
    pub position: [f64; 3],
    pub area: f64,
    pub e: [Complex; 3],
    pub h: [Complex; 3],
}
pub fn box_surface(mesh: &Mesh, inset: usize) -> Result<Vec<Surface>, String> {
    let dims = mesh.dims();
    if dims.iter().any(|&n| n <= 2 * inset + 2) {
        return Err("FDTD mesh is too small for the far-field box".into());
    }
    let mut out = Vec::new();
    for axis in 0..3 {
        let a = (axis + 1) % 3;
        let b = (axis + 2) % 3;
        for sign in [-1, 1] {
            let fixed = if sign < 0 {
                inset
            } else {
                dims[axis] - 1 - inset
            };
            for i in inset..dims[a] - 1 - inset {
                for j in inset..dims[b] - 1 - inset {
                    let mut xyz = [0; 3];
                    xyz[axis] = fixed;
                    xyz[a] = i;
                    xyz[b] = j;
                    let position = std::array::from_fn(|q| {
                        if q == axis {
                            mesh.axes[q][xyz[q]]
                        } else {
                            (mesh.axes[q][xyz[q]] + mesh.axes[q][xyz[q] + 1]) / 2.
                        }
                    });
                    out.push(Surface {
                        xyz,
                        axis,
                        sign,
                        position,
                        area: mesh.step(a, i) * mesh.step(b, j),
                        e: [Complex::default(); 3],
                        h: [Complex::default(); 3],
                    });
                }
            }
        }
    }
    Ok(out)
}
#[allow(clippy::too_many_arguments)]
pub fn accumulate(
    points: &mut [Surface],
    e: &[[f32; 3]],
    h: &[[f32; 3]],
    next: &[[f32; 3]],
    prev: &[[f32; 3]],
    next_h: &[[f32; 3]],
    start: usize,
    mesh: &Mesh,
    ep: Complex,
    hp: Complex,
) {
    let [nx, ny, _] = mesh.dims();
    let plane = nx * ny;
    let end = start + h.len() / plane;
    let magnetic = |p: [usize; 3], q: usize| {
        if p[2] < start {
            prev[p[1] * nx + p[0]][q] as f64
        } else if p[2] >= end {
            next_h[p[1] * nx + p[0]][q] as f64
        } else {
            h[((p[2] - start) * ny + p[1]) * nx + p[0]][q] as f64
        }
    };
    for s in points {
        let [i, j, k] = s.xyz;
        let l = ((k - start) * ny + j) * nx + i;
        for q in 0..3 {
            if q == s.axis {
                continue;
            }
            let other = 3 - q - s.axis;
            let e1 = if other == 2 && l + plane >= e.len() {
                next[j * nx + i][q]
            } else {
                e[l + [1, nx, plane][other]][q]
            };
            let mut up = s.xyz;
            up[q] += 1;
            let mut down = s.xyz;
            down[s.axis] -= 1;
            let mut down_up = down;
            down_up[q] += 1;
            let current = (magnetic(s.xyz, q) + magnetic(up, q)) / 2.;
            let previous = (magnetic(down, q) + magnetic(down_up, q)) / 2.;
            let before = mesh.step(s.axis, s.xyz[s.axis] - 1);
            let after = mesh.step(s.axis, s.xyz[s.axis]);
            s.e[q] += ep * ((e[l][q] as f64 + e1 as f64) / 2.);
            s.h[q] += hp * ((current * before + previous * after) / (before + after) / ETA0);
        }
    }
}
fn cross(a: [Complex; 3], b: [f64; 3]) -> [Complex; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub struct FarField {
    pub power: f64,
    pub directivity: f64,
    pub intensity: Vec<f64>,
    pub peak: usize,
}
pub fn calculate(points: &[Surface], f: f64, threads: usize) -> Result<FarField, String> {
    let wave = TAU * f / C0;
    // Equivalence currents and areas are independent of observation direction.
    let currents: Vec<_> = points
        .iter()
        .map(|s| {
            let mut normal = [0.; 3];
            normal[s.axis] = 1.;
            let hc = cross(s.h, normal);
            let ec = cross(s.e, normal);
            let weight = s.area * s.sign as f64;
            (
                s.position,
                hc.map(|v| v * (-weight)),
                ec.map(|v| v * weight),
            )
        })
        .collect();
    let workers = threads.max(1).min(37);
    let chunks = std::thread::scope(|scope| {
        let mut handles = vec![];
        for worker in 0..workers {
            let lo = 37 * worker / workers;
            let hi = 37 * (worker + 1) / workers;
            let currents = &currents;
            handles.push(scope.spawn(move || {
                let mut values = Vec::new();
                for it in lo..hi {
                    let theta = (it as f64 * 5.).to_radians();
                    for ip in 0..72 {
                        let phi = (ip as f64 * 5.).to_radians();
                        let r = [
                            theta.sin() * phi.cos(),
                            theta.sin() * phi.sin(),
                            theta.cos(),
                        ];
                        let mut n = [Complex::default(); 3];
                        let mut l = n;
                        for &(position, j, m) in currents {
                            let phase = Complex::phase(
                                wave * (0..3).map(|q| r[q] * position[q]).sum::<f64>(),
                            );
                            for q in 0..3 {
                                n[q] += j[q] * phase;
                                l[q] += m[q] * phase;
                            }
                        }
                        let dot = (0..3).fold(Complex::default(), |sum, q| sum + n[q] * r[q]);
                        let lc = cross(l, r);
                        let norm: f64 = (0..3)
                            .map(|q| ((n[q] - dot * r[q]) * ETA0 + lc[q]).norm())
                            .sum();
                        values.push(wave * wave / (32. * PI * PI * ETA0) * norm);
                    }
                }
                values
            }));
        }
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    let intensity: Vec<_> = chunks.into_iter().flatten().collect();
    let peak = intensity
        .iter()
        .enumerate()
        .max_by(|(ia, a), (ib, b)| a.total_cmp(b).then_with(|| ib.cmp(ia)))
        .unwrap()
        .0;
    let da = 5f64.to_radians();
    let power: f64 = intensity
        .iter()
        .enumerate()
        .map(|(i, u)| u * ((i / 72) as f64 * 5.).to_radians().sin() * da * da)
        .sum();
    if !power.is_finite() || power <= 0. {
        return Err("FDTD far-field has no radiated power".into());
    }
    Ok(FarField {
        power,
        directivity: 4. * PI * intensity[peak] / power,
        intensity,
        peak,
    })
}
