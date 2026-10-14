use super::{engine::Coeff, geometry::Feed, mesh::Mesh, Complex, EPS0};
use std::f64::consts::TAU;

#[derive(Clone)]
pub struct Edge {
    pub xyz: [usize; 3],
    pub axis: usize,
    pub length: f64,
    pub drive: f32,
}
pub struct Port {
    pub edges: Vec<Edge>,
    pub resistance: f64,
    pub length: f64,
    pub sign: f64,
    pub parallel: usize,
}
impl Port {
    pub fn new(mesh: &Mesh, feed: &Feed, resistance: f64) -> Result<Self, String> {
        let start = std::array::from_fn(|q| mesh.nearest(q, feed.start[q] * 1e-3));
        let stop: [usize; 3] = std::array::from_fn(|q| mesh.nearest(q, feed.stop[q] * 1e-3));
        let axis = feed.axis;
        let length = mesh.axes[axis][stop[axis]] - mesh.axes[axis][start[axis]];
        if length <= 0. {
            return Err("antenna feed has no mesh cells".into());
        }
        let edges = (start[axis]..stop[axis])
            .map(|i| {
                let mut xyz = start;
                xyz[axis] = i;
                Edge {
                    xyz,
                    axis,
                    length: mesh.step(axis, i),
                    drive: 0.,
                }
            })
            .collect();
        Ok(Self {
            edges,
            resistance,
            length,
            sign: feed.sign,
            parallel: 1,
        })
    }
    pub fn stamp(&mut self, mesh: &Mesh, coeff: &mut [[Coeff; 3]]) -> Result<(), String> {
        let [nx, ny, _] = mesh.dims();
        for e in &mut self.edges {
            let c = &mut coeff[(e.xyz[2] * ny + e.xyz[1]) * nx + e.xyz[0]][e.axis];
            if c.curl == 0. {
                return Err("antenna feed crosses copper; move the feed into a clear gap".into());
            }
            let area = (0..3)
                .filter(|&q| q != e.axis)
                .map(|q| mesh.dual(q, e.xyz[q]))
                .product::<f64>()
                * self.parallel as f64;
            // Implicit trapezoidal conductance: each edge receives its share of the series 50-ohm source.
            let old_curl = c.curl as f64;
            let extra = old_curl * super::ETA0 * self.length / (2. * self.resistance * area);
            c.decay = ((c.decay as f64 - extra) / (1. + extra)) as f32;
            c.curl = (old_curl / (1. + extra)) as f32;
            e.drive = (c.curl as f64 * super::ETA0 / (self.resistance * area) * self.sign) as f32;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct Pulse {
    pub f0: f64,
    pub width: f64,
    pub delay: f64,
}
impl Pulse {
    pub fn new(f0: f64, span: f64) -> Self {
        let width = std::f64::consts::LN_10.sqrt() / (std::f64::consts::PI * span / 2.);
        Self {
            f0,
            width,
            delay: 3. * width,
        }
    }
    pub fn value(self, t: f64) -> f64 {
        let u = t - self.delay;
        (-(u / self.width).powi(2)).exp() * (TAU * self.f0 * u).cos()
    }
}
pub fn dft(samples: &[f64], dt: f64, offset: f64, f: f64) -> Complex {
    let rot = Complex::phase(-TAU * f * dt);
    let mut phase = Complex::phase(-TAU * f * dt * offset);
    let mut sum = Complex::default();
    for &v in samples {
        sum += phase * v;
        phase = phase * rot;
    }
    sum * dt
}
pub struct Spectrum {
    pub f: Vec<f64>,
    pub s: Vec<Complex>,
    pub zin: Vec<Complex>,
    pub db: Vec<f64>,
    pub v0: Complex,
    pub i0: Complex,
}
pub fn spectrum(v: &[f64], i: &[f64], dt: f64, f0: f64, span: f64) -> Result<Spectrum, String> {
    let mut out = Spectrum {
        f: vec![],
        s: vec![],
        zin: vec![],
        db: vec![],
        v0: Complex::default(),
        i0: Complex::default(),
    };
    for k in 0..=400 {
        let f = f0 - span / 2. + span * k as f64 / 400.;
        let voltage = dft(v, dt, 1., f);
        let current = dft(i, dt, 0.5, f);
        let inc = (voltage + current * 50.) * 0.5;
        let reflected = (voltage - current * 50.) * 0.5;
        if inc.norm() < 1e-60 || current.norm() < 1e-70 {
            return Err("FDTD port has no measurable response".into());
        }
        let s = reflected / inc;
        let z = voltage / current;
        out.f.push(f);
        out.s.push(s);
        out.zin.push(z);
        out.db.push(20. * s.norm().sqrt().max(1e-12).log10());
        if k == 200 {
            out.v0 = voltage;
            out.i0 = current;
        }
    }
    Ok(out)
}
pub fn material(er: f64, sigma: f64, dt: f64) -> Coeff {
    let loss = sigma * dt / (2. * EPS0 * er);
    Coeff {
        decay: ((1. - loss) / (1. + loss)) as f32,
        curl: (dt * super::C0 / er / (1. + loss)) as f32,
    }
}
