//! SPICE3 device equations at the default nominal temperature, 27 C.
use std::{collections::BTreeMap, ops::{Add, Div, Mul, Neg, Sub}};

// shortcut: fixed 27 C nominal temperature, add .temp and temperature scaling for temperature sweeps.
pub(super) const VT: f64 = 8.617333262145e-5 * 300.15;

/// Four terminal derivatives keep the device Jacobians analytic, including stored charge.
#[derive(Clone, Copy, Debug)]
pub(super) struct Jet { pub v: f64, pub d: [f64; 4] }
impl Jet {
    pub fn constant(v: f64) -> Self { Self { v, d: [0.; 4] } }
    fn variable(v: f64, terminal: usize) -> Self { let mut x = Self::constant(v); x.d[terminal] = 1.; x }
    fn chain(self, value: f64, slope: f64) -> Self { Self { v: value, d: self.d.map(|v| v * slope) } }
    fn exp(self) -> Self {
        let e = self.v.min(80.).exp();
        // A linear continuation prevents overflow during rejected Newton trials.
        self.chain(e * (1. + (self.v - 80.).max(0.)), e)
    }
    fn ln(self) -> Self { self.chain(self.v.ln(), 1. / self.v) }
    fn powf(self, power: f64) -> Self { self.chain(self.v.powf(power), power * self.v.powf(power - 1.)) }
    fn sqrt(self) -> Self { self.powf(0.5) }
}
impl Add for Jet { type Output = Self; fn add(self, b: Self) -> Self { Self { v: self.v + b.v, d: std::array::from_fn(|i| self.d[i] + b.d[i]) } } }
impl Sub for Jet { type Output = Self; fn sub(self, b: Self) -> Self { self + -b } }
impl Neg for Jet { type Output = Self; fn neg(self) -> Self { Self { v: -self.v, d: self.d.map(|v| -v) } } }
impl Mul for Jet { type Output = Self; fn mul(self, b: Self) -> Self { Self { v: self.v * b.v, d: std::array::from_fn(|i| self.d[i] * b.v + self.v * b.d[i]) } } }
impl Div for Jet { type Output = Self; fn div(self, b: Self) -> Self { Self { v: self.v / b.v, d: std::array::from_fn(|i| (self.d[i] - self.v / b.v * b.d[i]) / b.v) } } }
impl Add<f64> for Jet { type Output = Self; fn add(self, b: f64) -> Self { self + Self::constant(b) } }
impl Sub<f64> for Jet { type Output = Self; fn sub(self, b: f64) -> Self { self + -b } }
impl Mul<f64> for Jet { type Output = Self; fn mul(self, b: f64) -> Self { Self { v: self.v * b, d: self.d.map(|v| v * b) } } }
impl Div<f64> for Jet { type Output = Self; fn div(self, b: f64) -> Self { self * (1. / b) } }

#[derive(Clone)]
pub(super) struct Model { kind: u8, polarity: f64, p: BTreeMap<String, f64>, w: f64, l: f64 }
impl Model {
    pub fn parse(kind: &str, params: BTreeMap<String, f64>) -> Result<Self, String> {
        let (kind, polarity, defaults): (u8, f64, &[(&str, f64)]) = match kind.to_ascii_lowercase().as_str() {
            "d" => (b'd', 1., &[("is", 1e-14), ("n", 1.), ("rs", 0.), ("cjo", 0.), ("vj", 1.), ("m", 0.5), ("tt", 0.), ("bv", f64::INFINITY), ("ibv", 1e-3)]),
            "npn" | "pnp" => (b'q', if kind.eq_ignore_ascii_case("pnp") { -1. } else { 1. }, &[
                ("is", 1e-16), ("bf", 100.), ("br", 1.), ("nf", 1.), ("nr", 1.), ("vaf", f64::INFINITY), ("var", f64::INFINITY),
                ("ikf", f64::INFINITY), ("rb", 0.), ("rc", 0.), ("re", 0.), ("cje", 0.), ("cjc", 0.), ("tf", 0.), ("tr", 0.)]),
            "nmos" | "pmos" => (b'm', if kind.eq_ignore_ascii_case("pmos") { -1. } else { 1. }, &[
                ("level", 1.), ("vto", 0.), ("kp", 2e-5), ("lambda", 0.), ("gamma", 0.), ("phi", 0.6),
                ("cgso", 0.), ("cgdo", 0.), ("cbd", 0.), ("cbs", 0.)]),
            _ => return Err(format!("unknown model type {kind}")),
        };
        let mut p: BTreeMap<String, f64> = defaults.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        for (key, value) in params {
            if !p.contains_key(&key) { return Err(format!("unsupported model parameter {key}")); }
            let positive = ["is", "n", "vj", "bv", "ibv", "bf", "br", "nf", "nr", "phi"].contains(&key.as_str());
            if (positive && value <= 0.) || (!positive && key != "vto" && value < 0.)
                || (key == "m" && value > 1.) || (key == "level" && value != 1.) {
                return Err(format!("invalid model parameter {key}"));
            }
            // SPICE disables Early/high-injection effects when their parameters are zero.
            p.insert(key.clone(), if value == 0. && ["vaf", "var", "ikf"].contains(&key.as_str()) { f64::INFINITY } else { value });
        }
        Ok(Self { kind, polarity, p, w: 100e-6, l: 100e-6 })
    }
    fn get(&self, key: &str) -> f64 { self.p[key] }
    pub fn matches(&self, kind: u8) -> bool { self.kind == kind }
    pub fn terminal_count(&self) -> usize { match self.kind { b'd' => 2, b'q' => 3, _ => 4 } }
    pub fn instance(&mut self, p: &BTreeMap<String, f64>) -> Result<(), String> {
        for (key, value) in p {
            if self.kind != b'm' || !["w", "l"].contains(&key.as_str()) { return Err(format!("unsupported instance parameter {key}")); }
            if *value <= 0. { return Err(format!("{key} must be positive")); }
            if key == "w" { self.w = *value; } else { self.l = *value; }
        }
        Ok(())
    }
    pub fn resistances(&self) -> Vec<(usize, f64)> {
        match self.kind { b'd' => vec![(0, self.get("rs"))], b'q' => vec![(0, self.get("rc")), (1, self.get("rb")), (2, self.get("re"))], _ => vec![] }
    }
    pub fn evaluate(&self, mut v: [f64; 4], old: [f64; 4], gmin: f64, limit: bool) -> Evaluation {
        let proposed = v;
        let p = self.polarity;
        if limit {
            match self.kind {
                b'd' => {
                    let vt = self.get("n") * VT;
                    let breakdown = self.breakdown();
                    let junction = v[0] - v[1];
                    let previous = old[0] - old[1];
                    let limited = if breakdown.is_finite() && junction < -breakdown {
                        -pnjlim(-junction - breakdown, -previous - breakdown, vt, self.get("is")) - breakdown
                    } else { pnjlim(junction, previous, vt, self.get("is")) };
                    v[0] = v[1] + limited;
                }
                b'q' => {
                    let be = pnjlim(p * (v[1] - v[2]), p * (old[1] - old[2]), self.get("nf") * VT, self.get("is"));
                    let bc = pnjlim(p * (v[1] - v[0]), p * (old[1] - old[0]), self.get("nr") * VT, self.get("is"));
                    v[2] = v[1] - p * be; v[0] = v[1] - p * bc;
                }
                _ => {
                    let source = if p * (v[0] - v[2]) >= 0. { 2 } else { 0 };
                    let gs = fetlim(p * (v[1] - v[source]), p * (old[1] - old[source]), p * self.get("vto"));
                    v[1] = v[source] + p * gs;
                    let bs = pnjlim(p * (v[3] - v[source]), p * (old[3] - old[source]), VT, 1e-14);
                    v[3] = v[source] + p * bs;
                    let drain = 2 - source;
                    let ds = p * (v[drain] - v[source]);
                    let old_ds = p * (old[drain] - old[source]);
                    // Drain limiting keeps the square-law Jacobian useful across mode changes.
                    let ds = if old_ds >= 3.5 {
                        if ds > old_ds { ds.min(3. * old_ds + 2.) } else if ds < 3.5 { ds.max(2.) } else { ds }
                    } else if ds > old_ds { ds.min(4.) } else { ds.max(-0.5) };
                    v[drain] = v[source] + p * ds;
                }
            }
        }
        let x: [Jet; 4] = std::array::from_fn(|i| Jet::variable(v[i], i));
        let zero = Jet::constant(0.);
        let mut current = [zero; 4];
        let mut charge = [zero; 4];
        match self.kind {
            b'd' => {
                let voltage = x[0] - x[1];
                let forward = ((voltage / (self.get("n") * VT)).exp() - 1.) * self.get("is");
                let mut i = forward + voltage * gmin;
                if self.get("bv").is_finite() { i = i - ((-voltage - self.breakdown()) / (self.get("n") * VT)).exp() * self.get("is"); }
                pair(&mut current, 0, 1, i);
                pair(&mut charge, 0, 1, depletion(voltage, self.get("cjo"), self.get("vj"), self.get("m")) + forward * self.get("tt"));
            }
            b'q' => {
                let be = (x[1] - x[2]) * p;
                let bc = (x[1] - x[0]) * p;
                let forward = ((be / (self.get("nf") * VT)).exp() - 1.) * self.get("is");
                let reverse = ((bc / (self.get("nr") * VT)).exp() - 1.) * self.get("is");
                let denominator = Jet::constant(1.) - bc / self.get("vaf") - be / self.get("var");
                let q1 = Jet::constant(1.) / if denominator.v > 1e-12 { denominator } else { Jet::constant(1e-12) };
                let q2 = forward / self.get("ikf");
                let qb = q1 * ((q2 * 4. + 1.).sqrt() + 1.) * 0.5;
                let transport = (forward - reverse) / qb;
                pair(&mut current, 0, 2, transport * p);
                pair(&mut current, 1, 2, (forward / self.get("bf") + be * gmin) * p);
                pair(&mut current, 1, 0, (reverse / self.get("br") + bc * gmin) * p);
                pair(&mut charge, 1, 2, (depletion(be, self.get("cje"), 0.75, 0.33) + forward / qb * self.get("tf")) * p);
                pair(&mut charge, 1, 0, (depletion(bc, self.get("cjc"), 0.75, 0.33) + reverse * self.get("tr")) * p);
            }
            _ => {
                let (drain, source) = if p * (v[0] - v[2]) >= 0. { (0, 2) } else { (2, 0) };
                let ds = (x[drain] - x[source]) * p;
                let gs = (x[1] - x[source]) * p;
                let bs = (x[3] - x[source]) * p;
                let phi = self.get("phi");
                let root = if bs.v <= 0. { (Jet::constant(phi) - bs).sqrt() } else { Jet::constant(phi.sqrt()) - bs / (2. * phi.sqrt()) };
                let threshold = (root - phi.sqrt()) * self.get("gamma") + p * self.get("vto");
                let overdrive = gs - threshold;
                if overdrive.v > 0. {
                    let beta = self.get("kp") * self.w / self.l;
                    let i = if ds.v < overdrive.v { (overdrive - ds * 0.5) * ds * beta } else { overdrive * overdrive * (beta * 0.5) };
                    pair(&mut current, drain, source, i * (ds * self.get("lambda") + 1.) * p);
                }
                for terminal in [0, 2] {
                    let bv = (x[3] - x[terminal]) * p;
                    pair(&mut current, 3, terminal, (((bv / VT).exp() - 1.) * 1e-14 + bv * gmin) * p);
                    let capacitance = self.get(if terminal == 0 { "cbd" } else { "cbs" });
                    pair(&mut charge, 3, terminal, depletion(bv, capacitance, 0.8, 0.5) * p);
                }
                pair(&mut charge, 1, 2, (x[1] - x[2]) * (self.get("cgso") * self.w));
                pair(&mut charge, 1, 0, (x[1] - x[0]) * (self.get("cgdo") * self.w));
            }
        }
        Evaluation { voltage: v, current, charge, limited: proposed.iter().zip(v).any(|(a, b)| (a - b).abs() > 1e-12) }
    }
    fn breakdown(&self) -> f64 { self.get("bv") - self.get("n") * VT * (self.get("ibv") / self.get("is")).ln() }
}

pub(super) struct Evaluation { pub voltage: [f64; 4], pub current: [Jet; 4], pub charge: [Jet; 4], pub limited: bool }
fn pair(values: &mut [Jet; 4], a: usize, b: usize, value: Jet) { values[a] = values[a] + value; values[b] = values[b] - value; }

fn depletion(v: Jet, c: f64, potential: f64, grading: f64) -> Jet {
    if c == 0. { return Jet::constant(0.); }
    let fc = 0.5;
    let integral = |x: Jet| if grading == 1. { -(Jet::constant(1.) - x / potential).ln() * (c * potential) }
        else { (Jet::constant(1.) - (Jet::constant(1.) - x / potential).powf(1. - grading)) * (c * potential / (1. - grading)) };
    if v.v < fc * potential { integral(v) }
    else {
        let qfc = integral(Jet::constant(fc * potential));
        let cfc = c * (1. - fc).powf(-grading);
        let delta = v - fc * potential;
        qfc + delta * cfc + delta * delta * (0.5 * cfc * grading / (potential * (1. - fc)))
    }
}

fn pnjlim(new: f64, old: f64, vt: f64, saturation: f64) -> f64 {
    let critical = vt * (vt / (2f64.sqrt() * saturation)).ln();
    if new > critical && (new - old).abs() > 2. * vt {
        if old > 0. {
            let change = (new - old) / vt;
            if change > 0. { old + vt * (2. + (change - 2.).ln()) }
            else { old - vt * (2. + (2. - change).ln()) }
        } else { vt * (new / vt).ln() }
    } else if new < 0. {
        new.max(if old > 0. { -1. - old } else { 2. * old - 1. })
    } else { new }
}

fn fetlim(new: f64, old: f64, threshold: f64) -> f64 {
    let high = 2. * (old - threshold).abs() + 2.;
    let low = (old - threshold).abs() + 1.;
    if old < threshold {
        if new <= old { new.max(old - high) }
        else if new <= threshold + 0.5 { new.min(old + low) }
        else { threshold + 0.5 }
    } else if old < threshold + 3.5 {
        if new <= old { new.max(threshold - 0.5) } else { new.min(threshold + 4.) }
    } else if new >= old { new.min(old + high) }
    else if new >= threshold + 3.5 { new.max(old - low) }
    else { new.max(threshold + 2.) }
}
