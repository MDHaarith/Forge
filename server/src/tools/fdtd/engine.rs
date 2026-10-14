//! Nonuniform Yee curls with unsplit convolutional PML and persistent z-slab workers.
use super::{
    mesh::{Mesh, PML},
    nf2ff::Surface,
    port::{Edge, Port, Pulse},
    Complex, C0, EPS0, ETA0,
};
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Clone, Copy)]
pub struct Coeff {
    pub decay: f32,
    pub curl: f32,
}
#[derive(Clone, Copy, Default)]
struct Stretch {
    inv: f32,
    b: f32,
    c: f32,
}
impl Stretch {
    fn apply(self, d: f32, psi: &mut f32) -> f32 {
        if self.c == 0. {
            return d;
        }
        *psi = self.b * *psi + self.c * d;
        self.inv * d + *psi
    }
}
fn stretch(mesh: &Mesh, axis: usize, half: bool, pml: usize) -> Vec<Stretch> {
    let a = &mesh.axes[axis];
    let n = a.len();
    (0..n)
        .map(|i| {
            let pos = if half && i + 1 < n {
                (a[i] + a[i + 1]) / 2.
            } else {
                a[i]
            };
            let (depth, thickness) = if pml == 0 {
                (0., 1.)
            } else if pos < a[pml] {
                ((a[pml] - pos) / (a[pml] - a[0]), a[pml] - a[0])
            } else if pos > a[n - 1 - pml] {
                (
                    (pos - a[n - 1 - pml]) / (a[n - 1] - a[n - 1 - pml]),
                    a[n - 1] - a[n - 1 - pml],
                )
            } else {
                (0., 1.)
            };
            if depth <= 0. {
                return Stretch {
                    inv: 1.,
                    b: 1.,
                    c: 0.,
                };
            }
            let d = depth.min(1.);
            let kappa = 1. + 4. * d.powi(3);
            // Roden/Gedney CFS-CPML: m=3, kappa_max=5, target normal-incidence reflection 1e-8.
            let sigma = -(4. * 1e-8_f64.ln()) / (2. * ETA0 * thickness) * d.powi(3);
            let alpha = 0.05 * (1. - d) / (ETA0 * thickness);
            let b = (-(sigma / kappa + alpha) * mesh.dt / EPS0).exp();
            let c = sigma * (b - 1.) / (kappa * (sigma + kappa * alpha));
            Stretch {
                inv: (1. / kappa) as f32,
                b: b as f32,
                c: c as f32,
            }
        })
        .collect()
}
type HKernel = unsafe fn(
    &Grid,
    usize,
    usize,
    &[[f32; 3]],
    &[[f32; 3]],
    &[[f32; 3]],
    &mut [[f32; 3]],
    PmlRow<'_>,
);
type EKernel = unsafe fn(
    &Grid,
    usize,
    usize,
    &[[f32; 3]],
    &[[f32; 3]],
    &[[f32; 3]],
    &mut [[f32; 3]],
    PmlRow<'_>,
    &[[Coeff; 3]],
);
fn kernels() -> (HKernel, EKernel) {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            return (Slab::h_avx2, Slab::e_avx2);
        }
        if std::arch::is_x86_feature_detected!("avx512f") {
            return (Slab::h_avx512, Slab::e_avx512);
        }
    }
    (Slab::h_row, Slab::e_row)
}
struct Grid {
    dims: [usize; 3],
    forward: [Vec<f32>; 3],
    backward: [Vec<f32>; 3],
    cell: [Vec<f64>; 3],
    dual: [Vec<f64>; 3],
    h_pml: [Vec<Stretch>; 3],
    e_pml: [Vec<Stretch>; 3],
    dt: f64,
    periodic_y: bool,
    pml: usize,
    air_rows: Vec<bool>,
    h_index: [Vec<Option<usize>>; 3],
    e_index: [Vec<Option<usize>>; 3],
    h_count: [usize; 3],
    e_count: [usize; 3],
    h_kernel: HKernel,
    e_kernel: EKernel,
}
impl Grid {
    fn new(mesh: &Mesh, pml: usize) -> Self {
        let (h_kernel, e_kernel) = kernels();
        let mut grid = Self {
            air_rows: vec![],
            h_index: std::array::from_fn(|_| vec![]),
            e_index: std::array::from_fn(|_| vec![]),
            h_count: [0; 3],
            e_count: [0; 3],
            h_kernel,
            e_kernel,
            dims: mesh.dims(),
            dt: mesh.dt,
            periodic_y: mesh.periodic_y,
            pml,
            forward: std::array::from_fn(|q| {
                (0..mesh.axes[q].len())
                    .map(|i| (1. / mesh.step(q, i)) as f32)
                    .collect()
            }),
            backward: std::array::from_fn(|q| {
                (0..mesh.axes[q].len())
                    .map(|i| (1. / mesh.dual(q, i)) as f32)
                    .collect()
            }),
            cell: std::array::from_fn(|q| {
                (0..mesh.axes[q].len()).map(|i| mesh.step(q, i)).collect()
            }),
            dual: std::array::from_fn(|q| {
                (0..mesh.axes[q].len()).map(|i| mesh.dual(q, i)).collect()
            }),
            h_pml: std::array::from_fn(|q| {
                stretch(
                    mesh,
                    q,
                    true,
                    if mesh.periodic_y && q == 1 { 0 } else { pml },
                )
            }),
            e_pml: std::array::from_fn(|q| {
                stretch(
                    mesh,
                    q,
                    false,
                    if mesh.periodic_y && q == 1 { 0 } else { pml },
                )
            }),
        };
        for q in 0..3 {
            let mut count = 0;
            grid.h_index[q] = grid.h_pml[q]
                .iter()
                .map(|p| {
                    if p.c == 0. {
                        None
                    } else {
                        let i = count;
                        count += 1;
                        Some(i)
                    }
                })
                .collect();
            grid.h_count[q] = count;
            let mut count = 0;
            grid.e_index[q] = grid.e_pml[q]
                .iter()
                .map(|p| {
                    if p.c == 0. {
                        None
                    } else {
                        let i = count;
                        count += 1;
                        Some(i)
                    }
                })
                .collect();
            grid.e_count[q] = count;
        }
        grid
    }
}
// Only derivatives stretched by CPML need a convolution history.
struct Pml {
    x: Vec<[f32; 2]>,
    y: Vec<[f32; 2]>,
    z: Vec<[f32; 2]>,
    start: usize,
    x_count: usize,
    y_count: usize,
    z_offset: usize,
}
struct PmlRow<'a> {
    x: &'a mut [[f32; 2]],
    y: &'a mut [[f32; 2]],
    z: &'a mut [[f32; 2]],
}
impl Pml {
    fn new(g: &Grid, start: usize, end: usize, magnetic: bool) -> Self {
        let index = if magnetic { &g.h_index } else { &g.e_index };
        let count = if magnetic { g.h_count } else { g.e_count };
        let [nx, ny, _] = g.dims;
        let z_count = index[2][start..end].iter().filter(|v| v.is_some()).count();
        Self {
            x: vec![[0.; 2]; (end - start) * ny * count[0]],
            y: vec![[0.; 2]; (end - start) * nx * count[1]],
            z: vec![[0.; 2]; z_count * ny * nx],
            start,
            x_count: count[0],
            y_count: count[1],
            z_offset: index[2][..start].iter().filter(|v| v.is_some()).count(),
        }
    }
    fn row(&mut self, g: &Grid, k: usize, j: usize, magnetic: bool) -> PmlRow<'_> {
        let [nx, ny, _] = g.dims;
        let idx = if magnetic { &g.h_index } else { &g.e_index };
        let x = ((k - self.start) * ny + j) * self.x_count;
        let yr = idx[1][j].map_or(0..0, |slot| {
            (((k - self.start) * self.y_count + slot) * nx)
                ..(((k - self.start) * self.y_count + slot + 1) * nx)
        });
        let zr = idx[2][k].map_or(0..0, |slot| {
            ((slot - self.z_offset) * ny + j) * nx..((slot - self.z_offset) * ny + j + 1) * nx
        });
        PmlRow {
            x: &mut self.x[x..x + self.x_count],
            y: &mut self.y[yr],
            z: &mut self.z[zr],
        }
    }
    fn apply(
        &mut self,
        g: &Grid,
        magnetic: bool,
        p: [usize; 3],
        axis: usize,
        field: usize,
        value: f32,
    ) -> f32 {
        let coeff = if magnetic {
            g.h_pml[axis][p[axis]]
        } else {
            g.e_pml[axis][p[axis]]
        };
        if coeff.c == 0. {
            return coeff.inv * value;
        }
        let index = if magnetic { &g.h_index } else { &g.e_index };
        let slot = index[axis][p[axis]].unwrap();
        let [nx, ny, _] = g.dims;
        let history = match axis {
            0 => &mut self.x[((p[2] - self.start) * ny + p[1]) * self.x_count + slot],
            1 => &mut self.y[((p[2] - self.start) * self.y_count + slot) * nx + p[0]],
            _ => &mut self.z[((slot - self.z_offset) * ny + p[1]) * nx + p[0]],
        };
        let part = if axis == 2 {
            usize::from(field == 0)
        } else {
            usize::from(field != 2)
        };
        coeff.apply(value, &mut history[part])
    }
}
fn pair(coeff: Stretch, values: [f32; 2], history: &mut [[f32; 2]], index: usize) -> [f32; 2] {
    if coeff.c == 0. {
        return values.map(|v| coeff.inv * v);
    }
    let p = &mut history[index];
    [
        coeff.apply(values[0], &mut p[0]),
        coeff.apply(values[1], &mut p[1]),
    ]
}
struct Slab {
    start: usize,
    end: usize,
    e: Vec<[f32; 3]>,
    h: Vec<[f32; 3]>,
    ep: Pml,
    hp: Pml,
    coeff: Vec<[Coeff; 3]>,
    edges: Vec<Edge>,
    surface: Vec<Surface>,
    probes: Vec<(usize, [usize; 3], usize)>,
}
impl Slab {
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn h_avx2(
        g: &Grid,
        k: usize,
        j: usize,
        e: &[[f32; 3]],
        y: &[[f32; 3]],
        z: &[[f32; 3]],
        h: &mut [[f32; 3]],
        p: PmlRow<'_>,
    ) {
        Self::h_row(g, k, j, e, y, z, h, p);
    }
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn e_avx2(
        g: &Grid,
        k: usize,
        j: usize,
        h: &[[f32; 3]],
        y: &[[f32; 3]],
        z: &[[f32; 3]],
        e: &mut [[f32; 3]],
        p: PmlRow<'_>,
        c: &[[Coeff; 3]],
    ) {
        Self::e_row(g, k, j, h, y, z, e, p, c);
    }
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx512f")]
    unsafe fn h_avx512(
        g: &Grid,
        k: usize,
        j: usize,
        e: &[[f32; 3]],
        y: &[[f32; 3]],
        z: &[[f32; 3]],
        h: &mut [[f32; 3]],
        p: PmlRow<'_>,
    ) {
        Self::h_row(g, k, j, e, y, z, h, p);
    }
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx512f")]
    unsafe fn e_avx512(
        g: &Grid,
        k: usize,
        j: usize,
        h: &[[f32; 3]],
        y: &[[f32; 3]],
        z: &[[f32; 3]],
        e: &mut [[f32; 3]],
        p: PmlRow<'_>,
        c: &[[Coeff; 3]],
    ) {
        Self::e_row(g, k, j, h, y, z, e, p, c);
    }
    #[inline(always)]
    fn h_row(
        g: &Grid,
        k: usize,
        j: usize,
        e: &[[f32; 3]],
        y: &[[f32; 3]],
        z: &[[f32; 3]],
        h: &mut [[f32; 3]],
        psi: PmlRow<'_>,
    ) {
        let nx = g.dims[0];
        assert!(e.len() == nx && y.len() == nx && z.len() == nx && h.len() == nx);
        let xp = &g.h_pml[0][..nx];
        let dx = &g.forward[0][..nx];
        let py = g.h_pml[1][j];
        let pz = g.h_pml[2][k];
        let dy = g.forward[1][j];
        let dz = g.forward[2][k];
        let c = (C0 * g.dt) as f32;
        let (lo, hi) = if py.c == 0. && pz.c == 0. {
            (g.pml, nx - 1 - g.pml)
        } else {
            (0, 0)
        };
        // Separate the plain Yee loop so LLVM can vectorize it without per-cell CPML branches.
        for i in lo..hi {
            let v = e[i];
            let x = e[i + 1];
            let y = y[i];
            let z = z[i];

            h[i][0] -= c * ((y[2] - v[2]) * dy - (z[1] - v[1]) * dz);
            h[i][1] -= c * ((z[0] - v[0]) * dz - (x[2] - v[2]) * dx[i]);
            h[i][2] -= c * ((x[1] - v[1]) * dx[i] - (y[0] - v[0]) * dy);
        }
        for i in (0..lo).chain(hi..nx - 1) {
            let v = e[i];
            let x = e[i + 1];
            let y = y[i];
            let z = z[i];
            let px = xp[i];

            let vx = pair(
                px,
                [(x[2] - v[2]) * dx[i], (x[1] - v[1]) * dx[i]],
                psi.x,
                g.h_index[0][i].unwrap_or(0),
            );
            let vy = pair(py, [(y[2] - v[2]) * dy, (y[0] - v[0]) * dy], psi.y, i);
            let vz = pair(pz, [(z[1] - v[1]) * dz, (z[0] - v[0]) * dz], psi.z, i);
            let (yz, zy, zx, xz, xy, yx) = (vy[0], vz[0], vz[1], vx[0], vx[1], vy[1]);
            h[i][0] -= c * (yz - zy);
            h[i][1] -= c * (zx - xz);
            h[i][2] -= c * (xy - yx);
        }
    }
    #[inline(always)]
    fn e_row(
        g: &Grid,
        k: usize,
        j: usize,
        h: &[[f32; 3]],
        y: &[[f32; 3]],
        z: &[[f32; 3]],
        e: &mut [[f32; 3]],
        psi: PmlRow<'_>,
        cs: &[[Coeff; 3]],
    ) {
        let nx = g.dims[0];
        assert!(h.len() == nx && y.len() == nx && z.len() == nx && e.len() == nx && cs.len() == nx);
        let xp = &g.e_pml[0][..nx];
        let dx = &g.backward[0][..nx];
        let py = g.e_pml[1][j];
        let pz = g.e_pml[2][k];
        let dy = g.backward[1][j];
        let dz = g.backward[2][k];
        let (lo, hi) = if py.c == 0. && pz.c == 0. {
            (g.pml.max(1), (nx - g.pml).min(nx - 1))
        } else {
            (1, 1)
        };
        let air = g.air_rows[k * g.dims[1] + j];
        let vacuum = (C0 * g.dt) as f32;
        // Rows containing only the exact air coefficients need no coefficient loads or decay multiplies.
        if air {
            for i in lo..hi {
                let v = h[i];
                let x = h[i - 1];
                let y = y[i];
                let z = z[i];
                let old = e[i];
                e[i] = [
                    old[0] + vacuum * ((v[2] - y[2]) * dy - (v[1] - z[1]) * dz),
                    old[1] + vacuum * ((v[0] - z[0]) * dz - (v[2] - x[2]) * dx[i]),
                    old[2] + vacuum * ((v[1] - x[1]) * dx[i] - (v[0] - y[0]) * dy),
                ];
            }
        } else {
            for i in lo..hi {
                let v = h[i];
                let x = h[i - 1];
                let y = y[i];
                let z = z[i];
                let old = e[i];
                let c = cs[i];

                e[i] = [
                    c[0].decay * old[0] + c[0].curl * ((v[2] - y[2]) * dy - (v[1] - z[1]) * dz),
                    c[1].decay * old[1] + c[1].curl * ((v[0] - z[0]) * dz - (v[2] - x[2]) * dx[i]),
                    c[2].decay * old[2] + c[2].curl * ((v[1] - x[1]) * dx[i] - (v[0] - y[0]) * dy),
                ];
            }
        }
        for i in (1..lo).chain(hi..nx - 1) {
            let v = h[i];
            let x = h[i - 1];
            let y = y[i];
            let z = z[i];
            let px = xp[i];
            let old = e[i];
            let c = if air {
                [Coeff {
                    decay: 1.,
                    curl: vacuum,
                }; 3]
            } else {
                cs[i]
            };

            let vx = pair(
                px,
                [(v[2] - x[2]) * dx[i], (v[1] - x[1]) * dx[i]],
                psi.x,
                g.e_index[0][i].unwrap_or(0),
            );
            let vy = pair(py, [(v[2] - y[2]) * dy, (v[0] - y[0]) * dy], psi.y, i);
            let vz = pair(pz, [(v[1] - z[1]) * dz, (v[0] - z[0]) * dz], psi.z, i);
            let (yz, zy, zx, xz, xy, yx) = (vy[0], vz[0], vz[1], vx[0], vx[1], vy[1]);
            e[i] = [
                c[0].decay * old[0] + c[0].curl * (yz - zy),
                c[1].decay * old[1] + c[1].curl * (zx - xz),
                c[2].decay * old[2] + c[2].curl * (xy - yx),
            ];
        }
    }
    fn h_step(&mut self, g: &Grid, next: &[[f32; 3]]) {
        let [nx, ny, nz] = g.dims;
        let plane = nx * ny;
        let c = (C0 * g.dt) as f32;
        for k in self.start..self.end {
            for j in 0..ny {
                let base = ((k - self.start) * ny + j) * nx;
                let regular = j + 1 < ny && k + 1 < nz;
                if regular {
                    let z = if k + 1 == self.end {
                        &next[j * nx..(j + 1) * nx]
                    } else {
                        &self.e[base + plane..base + plane + nx]
                    };
                    // SAFETY: kernels() checks the CPU feature; the kernel body uses checked slices.
                    unsafe {
                        (g.h_kernel)(
                            g,
                            k,
                            j,
                            &self.e[base..base + nx],
                            &self.e[base + nx..base + 2 * nx],
                            z,
                            &mut self.h[base..base + nx],
                            self.hp.row(g, k, j, true),
                        );
                    }
                }
                for i in if regular { nx - 1..nx } else { 0..nx } {
                    let l = ((k - self.start) * ny + j) * nx + i;
                    let xyz = [i, j, k];
                    for q in 0..3 {
                        let a = (q + 1) % 3;
                        let b = (q + 2) % 3;
                        if (xyz[a] + 1 >= g.dims[a] && !(a == 1 && g.periodic_y))
                            || (xyz[b] + 1 >= g.dims[b] && !(b == 1 && g.periodic_y))
                        {
                            continue;
                        }
                        let forward = |axis: usize, field: usize| {
                            let v = if axis == 1 && j + 1 == ny && g.periodic_y {
                                self.e[l - j * nx][field]
                            } else if axis == 2 && k + 1 == self.end {
                                next[j * nx + i][field]
                            } else {
                                self.e[l + [1, nx, plane][axis]][field]
                            };
                            (v - self.e[l][field]) * g.forward[axis][xyz[axis]]
                        };
                        let da = forward(a, b);
                        let db = forward(b, a);
                        let da = self.hp.apply(g, true, xyz, a, b, da);
                        let db = self.hp.apply(g, true, xyz, b, a, db);
                        self.h[l][q] -= c * (da - db);
                    }
                }
            }
        }
    }
    fn e_step(&mut self, g: &Grid, prev: &[[f32; 3]], drive: f32) -> (f64, f64) {
        let [nx, ny, _] = g.dims;
        let plane = nx * ny;
        let mut old = 0.;
        for edge in &self.edges {
            let [i, j, k] = edge.xyz;
            old += self.e[((k - self.start) * ny + j) * nx + i][edge.axis] as f64 * edge.length;
        }
        for k in self.start..self.end {
            for j in 0..ny {
                let base = ((k - self.start) * ny + j) * nx;
                let regular = j > 0 && j + 1 < ny && k > 0 && k + 1 < g.dims[2];
                if regular {
                    let z = if k == self.start {
                        &prev[j * nx..(j + 1) * nx]
                    } else {
                        &self.h[base - plane..base - plane + nx]
                    };
                    // SAFETY: kernels() checks the CPU feature; the kernel body uses checked slices.
                    unsafe {
                        (g.e_kernel)(
                            g,
                            k,
                            j,
                            &self.h[base..base + nx],
                            &self.h[base - nx..base],
                            z,
                            &mut self.e[base..base + nx],
                            self.ep.row(g, k, j, false),
                            &self.coeff[base..base + nx],
                        );
                    }
                }
                let first = if regular { 1 } else { nx };
                for i in (0..first).chain(if regular { nx - 1..nx } else { nx..nx }) {
                    let l = ((k - self.start) * ny + j) * nx + i;
                    let xyz = [i, j, k];
                    for q in 0..3 {
                        let a = (q + 1) % 3;
                        let b = (q + 2) % 3;
                        let boundary = |axis: usize| {
                            !(axis == 1 && g.periodic_y)
                                && (xyz[axis] == 0 || xyz[axis] + 1 == g.dims[axis])
                        };
                        if (xyz[q] + 1 >= g.dims[q] && !(q == 1 && g.periodic_y))
                            || boundary(a)
                            || boundary(b)
                        {
                            self.e[l][q] = 0.;
                            continue;
                        }
                        let backward = |axis: usize, field: usize| {
                            let v = if axis == 1 && j == 0 && g.periodic_y {
                                self.h[l + (ny - 1) * nx][field]
                            } else if axis == 2 && k == self.start {
                                prev[j * nx + i][field]
                            } else {
                                self.h[l - [1, nx, plane][axis]][field]
                            };
                            (self.h[l][field] - v) * g.backward[axis][xyz[axis]]
                        };
                        let da = backward(a, b);
                        let db = backward(b, a);
                        let da = self.ep.apply(g, false, xyz, a, b, da);
                        let db = self.ep.apply(g, false, xyz, b, a, db);
                        let c = self.coeff[l][q];
                        self.e[l][q] = c.decay * self.e[l][q] + c.curl * (da - db);
                    }
                }
            }
        }
        let mut new = 0.;
        for edge in &self.edges {
            let [i, j, k] = edge.xyz;
            let e = &mut self.e[((k - self.start) * ny + j) * nx + i][edge.axis];
            *e += edge.drive * drive;
            new += *e as f64 * edge.length;
        }
        (old, new)
    }
    fn energy(&self, g: &Grid) -> Vec<f64> {
        let plane = g.dims[0] * g.dims[1];
        self.e
            .chunks_exact(plane)
            .zip(self.h.chunks_exact(plane))
            .zip(self.coeff.chunks_exact(plane))
            .enumerate()
            .map(|(z, ((es, hs), cs))| {
                let mut sum = 0.;
                for (l, ((e, h), c)) in es.iter().zip(hs).zip(cs).enumerate() {
                    let xyz = [l % g.dims[0], l / g.dims[0], z + self.start];
                    for q in 0..3 {
                        let er = if c[q].curl > 0. {
                            (C0 * g.dt / (c[q].curl as f64) * (1. + c[q].decay as f64) / 2.).max(1.)
                        } else {
                            1.
                        };
                        let a = (q + 1) % 3;
                        let b = (q + 2) % 3;
                        let ve = g.cell[q][xyz[q]] * g.dual[a][xyz[a]] * g.dual[b][xyz[b]];
                        let vh = g.dual[q][xyz[q]] * g.cell[a][xyz[a]] * g.cell[b][xyz[b]];
                        sum += er * (e[q] as f64).powi(2) * ve + (h[q] as f64).powi(2) * vh;
                    }
                }
                sum
            })
            .collect()
    }
}
pub struct Run {
    pub voltage: Vec<f64>,
    pub current: Vec<f64>,
    pub surface: Vec<Surface>,
    pub probes: Vec<Vec<f64>>,
    pub steps: usize,
    pub decay: f64,
}
pub struct Config {
    pub steps: usize,
    pub end: f64,
    pub threads: usize,
    pub pml: usize,
    pub pulse: Pulse,
    pub surface: bool,
    pub probes: Vec<([usize; 3], usize)>,
    #[cfg(test)]
    pub scalar: bool,
}
impl Config {
    pub fn antenna(f0: f64, span: f64, accurate: bool) -> Self {
        Self {
            steps: if accurate { 200_000 } else { 80_000 },
            end: if accurate { 1e-4 } else { 1e-3 },
            threads: std::thread::available_parallelism()
                .map_or(1, usize::from)
                .min(8),
            pml: PML,
            pulse: Pulse::new(f0, span),
            surface: true,
            probes: vec![],
            #[cfg(test)]
            scalar: false,
        }
    }
}
struct WorkerResult {
    v: Vec<f64>,
    i: Vec<f64>,
    surface: Vec<Surface>,
    probes: Vec<(usize, Vec<f64>)>,
}
pub fn run(
    mesh: &Mesh,
    coeff: Vec<[Coeff; 3]>,
    port: &Port,
    cfg: &Config,
    initial: Option<Vec<[f32; 3]>>,
) -> Result<Run, String> {
    if mesh
        .dims()
        .iter()
        .enumerate()
        .any(|(q, &n)| !(q == 1 && mesh.periodic_y) && cfg.pml * 2 + 2 >= n)
    {
        return Err("FDTD mesh is too small for CPML".into());
    }
    let mut g = Grid::new(mesh, cfg.pml);
    let vacuum = (C0 * mesh.dt) as f32;
    g.air_rows = coeff
        .chunks_exact(g.dims[0])
        .map(|row| {
            row.iter()
                .all(|cs| cs.iter().all(|c| c.decay == 1. && c.curl == vacuum))
        })
        .collect();
    #[cfg(test)]
    let g = {
        let mut g = g;
        if cfg.scalar {
            g.h_kernel = Slab::h_row;
            g.e_kernel = Slab::e_row;
        }
        g
    };
    let [nx, ny, nz] = g.dims;
    let plane = nx * ny;
    // Keep the voltage integral in one slab so sums do not depend on thread count.
    let port_lo = port.edges.iter().map(|e| e.xyz[2]).min().unwrap_or(nz);
    let port_hi = port.edges.iter().map(|e| e.xyz[2] + 1).max().unwrap_or(0);
    let requested = cfg.threads.max(1).min(nz);
    let mut units = Vec::new();
    let mut k = 0;
    while k < nz {
        let end = if k == port_lo { port_hi } else { k + 1 };
        let cost = (k..end)
            .map(|z| {
                if z < cfg.pml || z >= nz - cfg.pml {
                    1.6
                } else {
                    1.
                }
            })
            .sum::<f64>();
        units.push((k, end, cost));
        k = end;
    }
    // Minimize the heaviest slab, including CPML work; timing measurements never affect partitioning.
    let bins = |cap: f64| {
        let mut count = 1;
        let mut sum = 0.;
        for &(_, _, cost) in &units {
            if sum + cost > cap * (1. + 1e-12) {
                count += 1;
                sum = 0.;
            }
            sum += cost;
        }
        count
    };
    let mut lo = units.iter().map(|u| u.2).fold(0f64, f64::max);
    let mut hi = units.iter().map(|u| u.2).sum::<f64>();
    for _ in 0..64 {
        let mid = (lo + hi) / 2.;
        if bins(mid) <= requested {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let mut bounds = vec![0];
    let mut sum = 0.;
    for &(start, _, cost) in &units {
        if sum + cost > hi * (1. + 1e-12) {
            bounds.push(start);
            sum = 0.;
        }
        sum += cost;
    }
    bounds.push(nz);
    let workers = bounds.len() - 1;
    let fmax =
        cfg.pulse.f0 + std::f64::consts::LN_10.sqrt() / (std::f64::consts::PI * cfg.pulse.width);
    // shortcut: NF2FF uses >=64 samples per shortest RF period, at most 16 Yee steps apart; use every-step DFT for broader non-Gaussian drives.
    let nf_stride = [16, 8, 4, 2, 1]
        .into_iter()
        .find(|&s| mesh.dt * s as f64 * fmax <= 1. / 64.)
        .unwrap_or(1);
    let mut e_tx: Vec<Option<Sender<Vec<[f32; 3]>>>> = (0..workers).map(|_| None).collect();
    let mut e_rx: Vec<Option<Receiver<Vec<[f32; 3]>>>> = (0..workers).map(|_| None).collect();
    let mut h_tx: Vec<Option<Sender<Vec<[f32; 3]>>>> = (0..workers).map(|_| None).collect();
    let mut h_rx: Vec<Option<Receiver<Vec<[f32; 3]>>>> = (0..workers).map(|_| None).collect();
    for w in 1..workers {
        let (tx, rx) = mpsc::channel();
        e_tx[w] = Some(tx);
        e_rx[w - 1] = Some(rx);
        let (tx, rx) = mpsc::channel();
        h_tx[w - 1] = Some(tx);
        h_rx[w] = Some(rx);
    }
    let surfaces = if cfg.surface {
        super::nf2ff::box_surface(mesh, cfg.pml + 4)?
    } else {
        vec![]
    };
    let (monitor_tx, monitor_rx) = mpsc::channel();
    let mut stop_tx = Vec::new();
    let mut stop_rx = Vec::new();
    for _ in 0..workers {
        let (tx, rx) = mpsc::channel();
        stop_tx.push(tx);
        stop_rx.push(Some(rx));
    }
    let mut peak = 0f64;
    let mut decay = 1.;
    let mut steps = 0;
    let results = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for w in 0..workers {
            let start = bounds[w];
            let end = bounds[w + 1];
            let size = (end - start) * plane;
            let mut slab = Slab {
                start,
                end,
                e: initial.as_ref().map_or_else(
                    || vec![[0.; 3]; size],
                    |e| e[start * plane..end * plane].to_vec(),
                ),
                h: vec![[0.; 3]; size],
                ep: Pml::new(&g, start, end, false),
                hp: Pml::new(&g, start, end, true),
                coeff: coeff[start * plane..end * plane].to_vec(),
                edges: port
                    .edges
                    .iter()
                    .filter(|e| e.xyz[2] >= start && e.xyz[2] < end)
                    .cloned()
                    .collect(),
                surface: surfaces
                    .iter()
                    .filter(|s| s.xyz[2] >= start && s.xyz[2] < end)
                    .cloned()
                    .collect(),
                probes: cfg
                    .probes
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.0[2] >= start && p.0[2] < end)
                    .map(|(i, p)| (i, p.0, p.1))
                    .collect(),
            };
            let et = e_tx[w].take();
            let er = e_rx[w].take();
            let ht = h_tx[w].take();
            let hr = h_rx[w].take();
            let stop = stop_rx[w].take().unwrap();
            let monitor = monitor_tx.clone();
            let grid = &g;
            handles.push(scope.spawn(move || {
                let mut result = WorkerResult {
                    v: vec![],
                    i: vec![],
                    surface: vec![],
                    probes: slab.probes.iter().map(|p| (p.0, vec![])).collect(),
                };
                let zero = vec![[0.; 3]; plane];
                for step in 0..cfg.steps {
                    if let Some(tx) = &et {
                        tx.send(slab.e[..plane].to_vec()).unwrap();
                    }
                    let next = er
                        .as_ref()
                        .map_or_else(|| zero.clone(), |rx| rx.recv().unwrap());
                    slab.h_step(grid, &next);
                    if let Some(tx) = &ht {
                        tx.send(slab.h[size - plane..].to_vec()).unwrap();
                    }
                    let prev = hr
                        .as_ref()
                        .map_or_else(|| zero.clone(), |rx| rx.recv().unwrap());
                    let t = step as f64 * grid.dt;
                    if cfg.surface && step % nf_stride == 0 {
                        // Reuse the forward channel for the H halo after consuming its E halo.
                        if let Some(tx) = &et {
                            tx.send(slab.h[..plane].to_vec()).unwrap();
                        }
                        let next_h = er
                            .as_ref()
                            .map_or_else(|| zero.clone(), |rx| rx.recv().unwrap());
                        let ep = Complex::phase(-std::f64::consts::TAU * cfg.pulse.f0 * t)
                            * (grid.dt * nf_stride as f64);
                        let hp = Complex::phase(
                            -std::f64::consts::TAU * cfg.pulse.f0 * (t + grid.dt / 2.),
                        ) * (grid.dt * nf_stride as f64);
                        super::nf2ff::accumulate(
                            &mut slab.surface,
                            &slab.e,
                            &slab.h,
                            &next,
                            &prev,
                            &next_h,
                            start,
                            mesh,
                            ep,
                            hp,
                        );
                    }
                    let drive = cfg.pulse.value(t + grid.dt / 2.);
                    let (old, new) = slab.e_step(grid, &prev, drive as f32);
                    result.v.push(new * port.sign / port.parallel as f64);
                    let length: f64 = slab.edges.iter().map(|e| e.length).sum();
                    result.i.push(
                        (drive * length / port.length / port.parallel as f64
                            - (old + new) * port.sign / (2. * port.parallel as f64))
                            / port.resistance,
                    );
                    for ((_, p, q), (_, trace)) in slab.probes.iter().zip(&mut result.probes) {
                        let l = ((p[2] - start) * ny + p[1]) * nx + p[0];
                        trace.push(if *q < 3 {
                            slab.e[l][*q] as f64
                        } else {
                            slab.h[l][*q - 3] as f64 / ETA0
                        });
                    }
                    if (step + 1) % 64 == 0 || step + 1 == cfg.steps {
                        monitor.send((w, step + 1, slab.energy(grid))).unwrap();
                        if stop.recv().unwrap() {
                            break;
                        }
                    }
                }
                result.surface = slab.surface;
                result
            }));
        }
        loop {
            let mut rows: Vec<Option<Vec<f64>>> = (0..workers).map(|_| None).collect();
            for _ in 0..workers {
                let (w, count, energy) = monitor_rx.recv().unwrap();
                steps = count;
                rows[w] = Some(energy);
            }
            let energy: f64 = rows.iter().flat_map(|e| e.as_ref().unwrap()).copied().sum();
            peak = peak.max(energy);
            decay = if peak > 0. { energy / peak } else { 1. };
            let stop = !energy.is_finite()
                || (steps as f64 * mesh.dt > cfg.pulse.delay + 3. * cfg.pulse.width
                    && decay < cfg.end)
                || steps >= cfg.steps;
            if steps % 1024 == 0 || stop {
                eprintln!("FDTD step {steps}: energy decay {decay:.6e}");
            }
            for tx in &stop_tx {
                tx.send(stop).unwrap();
            }
            if stop {
                break;
            }
        }
        handles
            .into_iter()
            .map(|h| h.join().map_err(|_| "FDTD worker panicked".to_string()))
            .collect::<Result<Vec<_>, _>>()
    })?;
    if !decay.is_finite() {
        return Err("FDTD fields diverged".into());
    }
    let mut out = Run {
        voltage: vec![0.; steps],
        current: vec![0.; steps],
        surface: vec![],
        probes: vec![vec![]; cfg.probes.len()],
        steps,
        decay,
    };
    for r in results {
        for (v, x) in out.voltage.iter_mut().zip(r.v) {
            *v += x;
        }
        for (i, x) in out.current.iter_mut().zip(r.i) {
            *i += x;
        }
        out.surface.extend(r.surface);
        for (p, trace) in r.probes {
            out.probes[p] = trace;
        }
    }
    out.surface.sort_by_key(|s| (s.axis, s.sign, s.xyz));
    Ok(out)
}
