//! Native full-wave antenna simulation. All field and Fourier work is CPU-only.
mod engine;
pub mod geometry;
mod mesh;
mod nf2ff;
mod output;
mod port;
#[cfg(test)]
mod tests;

use serde_json::Value;
use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};
const C0: f64 = 299_792_458.;
const EPS0: f64 = 8.8541878128e-12;
const ETA0: f64 = 376.730313668;
#[derive(Clone, Copy, Default, Debug)]
struct Complex {
    re: f64,
    im: f64,
}
impl Complex {
    fn phase(a: f64) -> Self {
        let (im, re) = a.sin_cos();
        Self { re, im }
    }
    fn norm(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
    fn conj(self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
        }
    }
}
impl Add for Complex {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self {
            re: self.re + b.re,
            im: self.im + b.im,
        }
    }
}
impl Sub for Complex {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self {
            re: self.re - b.re,
            im: self.im - b.im,
        }
    }
}
impl AddAssign for Complex {
    fn add_assign(&mut self, b: Self) {
        *self = *self + b;
    }
}
impl SubAssign for Complex {
    fn sub_assign(&mut self, b: Self) {
        *self = *self - b;
    }
}
impl Mul for Complex {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        Self {
            re: self.re * b.re - self.im * b.im,
            im: self.re * b.im + self.im * b.re,
        }
    }
}
impl Mul<f64> for Complex {
    type Output = Self;
    fn mul(self, b: f64) -> Self {
        Self {
            re: self.re * b,
            im: self.im * b,
        }
    }
}
impl Div for Complex {
    type Output = Self;
    fn div(self, b: Self) -> Self {
        self * b.conj() * (1. / b.norm())
    }
}

pub struct Simulation {
    pub result: Value,
    pub artifacts: Vec<(String, String)>,
}
pub fn simulate(
    circuit: &Value,
    antenna: &str,
    f0: f64,
    span: f64,
    er: f64,
    tand: f64,
    accurate: bool,
) -> Result<Simulation, String> {
    let board = geometry::load(circuit, antenna)?;
    let mesh = mesh::build(&board, f0 - span / 2., f0 + span / 2., er, accurate)?;
    let cfg = engine::Config::antenna(f0, span, accurate);
    let [nx, ny, nz] = mesh.dims();
    eprintln!("Mesh {nx} x {ny} x {nz} = {} cells", mesh.cells());
    eprintln!(
        "Courant timestep {:.6e} s; {} requested threads",
        mesh.dt, cfg.threads
    );
    let mut coeff = materials(&mesh, &board, er, tand, f0);
    let mut feed = port::Port::new(&mesh, &board.feed, 50.)?;
    feed.stamp(&mesh, &mut coeff)?;
    let mut fields = engine::run(&mesh, coeff, &feed, &cfg, None)?;
    let mut spectrum = port::spectrum(&fields.voltage, &fields.current, mesh.dt, f0, span)?;
    // Normalize the pulse spectra to a 1 V harmonic Thevenin source so Prad and port powers are watts.
    let drive: Vec<_> = (0..fields.steps)
        .map(|n| cfg.pulse.value((n as f64 + 0.5) * mesh.dt))
        .collect();
    let excitation = port::dft(&drive, mesh.dt, 0.5, f0);
    if excitation.norm() < 1e-60 {
        return Err("FDTD excitation has no measurable spectrum".into());
    }
    spectrum.v0 = spectrum.v0 / excitation;
    spectrum.i0 = spectrum.i0 / excitation;
    for point in &mut fields.surface {
        for q in 0..3 {
            point.e[q] = point.e[q] / excitation;
            point.h[q] = point.h[q] / excitation;
        }
    }
    let far = nf2ff::calculate(&fields.surface, f0, cfg.threads)?;
    eprintln!(
        "NF2FF radiated power {:.6e} W for a 1 V source; Dmax {:.6}",
        far.power, far.directivity
    );
    eprintln!(
        "FDTD finished in {} steps; energy decay {:.6e}",
        fields.steps, fields.decay
    );
    output::artifacts(&board, &mesh, &spectrum, &far, f0, span)
}

fn materials(
    mesh: &mesh::Mesh,
    board: &geometry::Board,
    er: f64,
    tand: f64,
    f0: f64,
) -> Vec<[engine::Coeff; 3]> {
    // shortcut: diagonal/curved boundaries use Yee staircasing and narrow vias retain a PEC centre line, add conformal/subcell conductors when those geometries need finer accuracy.
    let [nx, ny, nz] = mesh.dims();
    let mut cell = vec![0.; mesh.cells()];
    let within: Vec<_> = (0..ny - 1)
        .flat_map(|j| (0..nx - 1).map(move |i| (i, j)))
        .map(|(i, j)| {
            geometry::inside(
                &board.outline,
                [
                    (mesh.axes[0][i] + mesh.axes[0][i + 1]) * 500.,
                    (mesh.axes[1][j] + mesh.axes[1][j + 1]) * 500.,
                ],
            )
        })
        .collect();
    for k in 0..nz - 1 {
        let z = (mesh.axes[2][k] + mesh.axes[2][k + 1]) * 500.;
        if z < 0. || z > board.thickness {
            continue;
        }
        for j in 0..ny - 1 {
            for i in 0..nx - 1 {
                if within[j * (nx - 1) + i] {
                    cell[(k * ny + j) * nx + i] = 1.;
                }
            }
        }
    }
    let sigma = std::f64::consts::TAU * f0 * EPS0 * er * tand;
    let air = port::material(1., 0., mesh.dt);
    let mut coeff = vec![[air; 3]; mesh.cells()];
    for k in 1..nz - 1 {
        for j in 1..ny - 1 {
            for i in 1..nx - 1 {
                let xyz = [i, j, k];
                let index = (k * ny + j) * nx + i;
                for q in 0..3 {
                    let a = (q + 1) % 3;
                    let b = (q + 2) % 3;
                    let mut sum = 0.;
                    let mut weight = 0.;
                    for da in [0, 1] {
                        for db in [0, 1] {
                            let mut p = xyz;
                            p[a] -= da;
                            p[b] -= db;
                            let w = mesh.step(a, p[a]) * mesh.step(b, p[b]);
                            sum += cell[(p[2] * ny + p[1]) * nx + p[0]] * w;
                            weight += w;
                        }
                    }
                    let fraction = sum / weight;
                    let eps = 1. + (er - 1.) * fraction;
                    let loss = sigma * fraction;
                    coeff[index][q] = port::material(eps, loss, mesh.dt);
                }
            }
        }
    }
    let pec = engine::Coeff {
        decay: 0.,
        curl: 0.,
    };
    for s in board.pours.iter().chain(&board.copper) {
        let k = mesh.nearest(2, s.z * 1e-3);
        let bounds = |axis| {
            let lo = s
                .outer
                .iter()
                .map(|p| p[axis] * 1e-3)
                .fold(f64::INFINITY, f64::min);
            let hi = s
                .outer
                .iter()
                .map(|p| p[axis] * 1e-3)
                .fold(f64::NEG_INFINITY, f64::max);
            mesh.nearest(axis, lo).saturating_sub(1)
                ..(mesh.nearest(axis, hi) + 2).min(mesh.dims()[axis] - 1)
        };
        for j in bounds(1) {
            for i in bounds(0) {
                for q in 0..2 {
                    let p = [
                        (mesh.axes[0][i] + if q == 0 { mesh.step(0, i) / 2. } else { 0. }) * 1e3,
                        (mesh.axes[1][j] + if q == 1 { mesh.step(1, j) / 2. } else { 0. }) * 1e3,
                    ];
                    if s.contains(p) {
                        coeff[(k * ny + j) * nx + i][q] = pec;
                    }
                }
            }
        }
    }
    for b in &board.barrels {
        let x = mesh.nearest(0, b.xy[0] * 1e-3);
        let y = mesh.nearest(1, b.xy[1] * 1e-3);
        let lo = mesh.nearest(2, b.z[0] * 1e-3);
        let hi = mesh.nearest(2, b.z[1] * 1e-3);
        for k in lo..hi {
            coeff[(k * ny + y) * nx + x][2] = pec;
        }
        for k in lo..=hi {
            for j in mesh
                .nearest(1, (b.xy[1] - b.radius) * 1e-3)
                .saturating_sub(1)
                ..(mesh.nearest(1, (b.xy[1] + b.radius) * 1e-3) + 2).min(ny - 1)
            {
                for i in mesh
                    .nearest(0, (b.xy[0] - b.radius) * 1e-3)
                    .saturating_sub(1)
                    ..(mesh.nearest(0, (b.xy[0] + b.radius) * 1e-3) + 2).min(nx - 1)
                {
                    for q in 0..3 {
                        let px = (mesh.axes[0][i] + if q == 0 { mesh.step(0, i) / 2. } else { 0. })
                            * 1e3;
                        let py = (mesh.axes[1][j] + if q == 1 { mesh.step(1, j) / 2. } else { 0. })
                            * 1e3;
                        if (px - b.xy[0]).hypot(py - b.xy[1]) <= b.radius && (q != 2 || k < hi) {
                            coeff[(k * ny + j) * nx + i][q] = pec;
                        }
                    }
                }
            }
        }
    }
    coeff
}
