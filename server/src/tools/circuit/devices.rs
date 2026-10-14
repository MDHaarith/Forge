//! SPICE3 device equations, temperature scaling, and small-signal noise sources.
use std::{collections::BTreeMap, ops::{Add, Div, Mul, Neg, Sub}};

pub(super) const VT: f64 = 8.617333262145e-5 * 300.15;
pub(super) const BOLTZMANN: f64 = 1.380649e-23;
const ELECTRON_CHARGE: f64 = 1.602176634e-19;

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
pub(super) struct Model { kind: u8, polarity: f64, p: BTreeMap<String, f64>, w: f64, l: f64, vt: f64 }
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
        match kind {
            b'd' => p.extend([("eg", 1.11), ("xti", 3.), ("fc", 0.5)].map(|(k, v)| (k.into(), v))),
            b'q' => p.extend([("eg", 1.11), ("xti", 3.), ("xtb", 0.), ("vje", 0.75), ("vjc", 0.75), ("mje", 0.33), ("mjc", 0.33), ("fc", 0.5)].map(|(k, v)| (k.into(), v))),
            _ => p.extend([("tox", 0.), ("cgbo", 0.), ("pb", 0.8), ("mj", 0.5), ("is", 1e-14), ("kf", 0.), ("af", 1.), ("ld", 0.)].map(|(k, v)| (k.into(), v))),
        }
        let kp_given = params.contains_key("kp");
        for (key, value) in params {
            if !p.contains_key(&key) && key != "tnom" { return Err(format!("unsupported model parameter {key}")); }
            let positive = ["is", "n", "vj", "vje", "vjc", "pb", "bv", "ibv", "bf", "br", "nf", "nr", "phi"].contains(&key.as_str());
            if (positive && value <= 0.) || (!positive && !["vto", "tnom", "xtb"].contains(&key.as_str()) && value < 0.)
                || (["m", "mje", "mjc", "mj"].contains(&key.as_str()) && value > 1.) || (key == "level" && value != 1.)
                || (key == "tnom" && value <= -273.15) || (key == "fc" && value >= 1.) {
                return Err(format!("invalid model parameter {key}"));
            }
            // SPICE disables Early/high-injection effects when their parameters are zero.
            p.insert(key.clone(), if value == 0. && ["vaf", "var", "ikf"].contains(&key.as_str()) { f64::INFINITY } else { value });
        }
        if kind == b'm' && !kp_given && p["tox"] > 0. { p.insert("kp".into(), 600. * 1e-4 * 3.9 * 8.854214871e-12 / p["tox"]); }
        Ok(Self { kind, polarity, p, w: 100e-6, l: 100e-6, vt: VT })
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
        if self.kind == b'm' && self.l <= 2. * self.get("ld") { return Err("effective channel length must be positive".into()); }
        Ok(())
    }
    pub fn resistances(&self) -> Vec<(usize, f64)> {
        match self.kind { b'd' => vec![(0, self.get("rs"))], b'q' => vec![(0, self.get("rc")), (1, self.get("rb")), (2, self.get("re"))], _ => vec![] }
    }
    pub fn temperature(&mut self, temp: f64, tnom: f64) -> Result<(), String> {
        let nominal = self.p.get("tnom").copied().unwrap_or(tnom) + 273.15;
        let kelvin = temp + 273.15;
        self.vt = if kelvin == 300.15 { VT } else { VT * kelvin / 300.15 };
        if kelvin == nominal { return Ok(()); }
        let ratio = kelvin / nominal;
        match self.kind {
            b'd' | b'q' => {
                let emission = if self.kind == b'd' { self.get("n") } else { 1. };
                let scale = (((ratio - 1.) * self.get("eg") / self.vt + self.get("xti") * ratio.ln()) / emission).exp();
                *self.p.get_mut("is").unwrap() *= scale;
                if self.kind == b'q' {
                    let beta = ratio.powf(self.get("xtb"));
                    *self.p.get_mut("bf").unwrap() *= beta; *self.p.get_mut("br").unwrap() *= beta;
                    // SPICE3 BJT junction preprocessing uses the operating-temperature potential shift for both factors.
                    self.scale_junction("cje", "vje", "mje", kelvin, nominal, potential_shift(kelvin));
                    self.scale_junction("cjc", "vjc", "mjc", kelvin, nominal, potential_shift(kelvin));
                } else { self.scale_junction("cjo", "vj", "m", kelvin, nominal, potential_shift(nominal)); }
            }
            _ => {
                let old_phi = self.get("phi");
                let phi = kelvin / 300.15 * (old_phi - potential_shift(nominal)) / (nominal / 300.15) + potential_shift(kelvin);
                if phi <= 0. { return Err("temperature adjusted PHI must be positive".into()); }
                let vto = self.get("vto") + 0.5 * (bandgap(nominal) - bandgap(kelvin))
                    + self.polarity * (0.5 * (phi - old_phi) + self.get("gamma") * (phi.sqrt() - old_phi.sqrt()));
                self.p.insert("phi".into(), phi); self.p.insert("vto".into(), vto);
                *self.p.get_mut("kp").unwrap() /= ratio.powf(1.5);
                *self.p.get_mut("is").unwrap() *= (-bandgap(kelvin) / self.vt + bandgap(nominal) / (VT * nominal / 300.15)).exp();
                let original = self.get("pb");
                self.scale_junction("cbd", "pb", "mj", kelvin, nominal, potential_shift(nominal));
                let scaled = self.get("pb"); self.p.insert("pb".into(), original);
                self.scale_junction("cbs", "pb", "mj", kelvin, nominal, potential_shift(nominal));
                self.p.insert("pb".into(), scaled);
            }
        }
        if self.p.iter().any(|(key, v)| !["bv", "vaf", "var", "ikf"].contains(&key.as_str()) && !v.is_finite()) {
            return Err("temperature adjusted model is not finite".into());
        }
        if ["vj", "vje", "vjc", "pb"].iter().any(|key| self.p.get(*key).is_some_and(|v| *v <= 0.)) { return Err("temperature adjusted junction potential must be positive".into()); }
        Ok(())
    }
    fn scale_junction(&mut self, cap: &str, potential: &str, grading: &str, temp: f64, nominal: f64, nominal_shift: f64) {
        let old = self.get(potential);
        let pbo = (old - nominal_shift) / (nominal / 300.15);
        let new = temp / 300.15 * pbo + potential_shift(temp);
        let factor = (1. + self.get(grading) * (4e-4 * (temp - 300.15) - (new - pbo) / pbo))
            / (1. + self.get(grading) * (4e-4 * (nominal - 300.15) - (old - pbo) / pbo));
        *self.p.get_mut(cap).unwrap() *= factor; self.p.insert(potential.into(), new);
    }
    pub fn evaluate(&self, mut v: [f64; 4], old: [f64; 4], gmin: f64, limit: bool) -> Evaluation {
        let proposed = v;
        let p = self.polarity;
        if limit {
            match self.kind {
                b'd' => {
                    let vt = self.get("n") * self.vt;
                    let breakdown = self.breakdown();
                    let junction = v[0] - v[1];
                    let previous = old[0] - old[1];
                    let limited = if breakdown.is_finite() && junction < -breakdown {
                        -pnjlim(-junction - breakdown, -previous - breakdown, vt, self.get("is")) - breakdown
                    } else { pnjlim(junction, previous, vt, self.get("is")) };
                    v[0] = v[1] + limited;
                }
                b'q' => {
                    let be = pnjlim(p * (v[1] - v[2]), p * (old[1] - old[2]), self.get("nf") * self.vt, self.get("is"));
                    let bc = pnjlim(p * (v[1] - v[0]), p * (old[1] - old[0]), self.get("nr") * self.vt, self.get("is"));
                    v[2] = v[1] - p * be; v[0] = v[1] - p * bc;
                }
                _ => {
                    let source = if p * (v[0] - v[2]) >= 0. { 2 } else { 0 };
                    let gs = fetlim(p * (v[1] - v[source]), p * (old[1] - old[source]), p * self.get("vto"));
                    v[1] = v[source] + p * gs;
                    let bs = pnjlim(p * (v[3] - v[source]), p * (old[3] - old[source]), self.vt, self.get("is"));
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
                let forward = ((voltage / (self.get("n") * self.vt)).exp() - 1.) * self.get("is");
                let mut i = forward + voltage * gmin;
                if self.get("bv").is_finite() { i = i - ((-voltage - self.breakdown()) / (self.get("n") * self.vt)).exp() * self.get("is"); }
                pair(&mut current, 0, 1, i);
                pair(&mut charge, 0, 1, depletion(voltage, self.get("cjo"), self.get("vj"), self.get("m"), self.get("fc")) + forward * self.get("tt"));
            }
            b'q' => {
                let be = (x[1] - x[2]) * p;
                let bc = (x[1] - x[0]) * p;
                let forward = ((be / (self.get("nf") * self.vt)).exp() - 1.) * self.get("is");
                let reverse = ((bc / (self.get("nr") * self.vt)).exp() - 1.) * self.get("is");
                let denominator = Jet::constant(1.) - bc / self.get("vaf") - be / self.get("var");
                let q1 = Jet::constant(1.) / if denominator.v > 1e-12 { denominator } else { Jet::constant(1e-12) };
                let q2 = forward / self.get("ikf");
                let qb = q1 * ((q2 * 4. + 1.).sqrt() + 1.) * 0.5;
                let transport = (forward - reverse) / qb;
                pair(&mut current, 0, 2, transport * p);
                pair(&mut current, 1, 2, (forward / self.get("bf") + be * gmin) * p);
                pair(&mut current, 1, 0, (reverse / self.get("br") + bc * gmin) * p);
                pair(&mut charge, 1, 2, (depletion(be, self.get("cje"), self.get("vje"), self.get("mje"), self.get("fc")) + forward / qb * self.get("tf")) * p);
                pair(&mut charge, 1, 0, (depletion(bc, self.get("cjc"), self.get("vjc"), self.get("mjc"), self.get("fc")) + reverse * self.get("tr")) * p);
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
                    let beta = self.get("kp") * self.w / (self.l - 2. * self.get("ld"));
                    let i = if ds.v < overdrive.v { (overdrive - ds * 0.5) * ds * beta } else { overdrive * overdrive * (beta * 0.5) };
                    pair(&mut current, drain, source, i * (ds * self.get("lambda") + 1.) * p);
                }
                for terminal in [0, 2] {
                    let bv = (x[3] - x[terminal]) * p;
                    pair(&mut current, 3, terminal, (((bv / self.vt).exp() - 1.) * self.get("is") + bv * gmin) * p);
                    let capacitance = self.get(if terminal == 0 { "cbd" } else { "cbs" });
                    pair(&mut charge, 3, terminal, depletion(bv, capacitance, self.get("pb"), self.get("mj"), 0.5) * p);
                }
                pair(&mut charge, 1, 2, (x[1] - x[2]) * (self.get("cgso") * self.w));
                pair(&mut charge, 1, 0, (x[1] - x[0]) * (self.get("cgdo") * self.w));
            }
        }
        Evaluation { voltage: v, current, charge, limited: proposed.iter().zip(v).any(|(a, b)| (a - b).abs() > 1e-12) }
    }
    pub fn small_signal(&self, v: [f64; 4], gmin: f64) -> Evaluation {
        let mut eval = self.evaluate(v, v, gmin, false);
        for (a, b, cap) in self.gate_caps(v) {
            pair(&mut eval.charge, a, b, (Jet::variable(v[a], a) - Jet::variable(v[b], b)) * cap);
        }
        eval
    }
    /// Meyer capacitances are differential values; do not differentiate C(V)*V.
    pub fn gate_caps(&self, v: [f64; 4]) -> Vec<(usize, usize, f64)> {
        if self.kind != b'm' || (self.get("tox") == 0. && self.get("cgbo") == 0.) { return vec![]; }
        let p = self.polarity;
        let (drain, source) = if p * (v[0] - v[2]) >= 0. { (0, 2) } else { (2, 0) };
        let ds = p * (v[drain] - v[source]);
        let gs = p * (v[1] - v[source]);
        let bs = p * (v[3] - v[source]);
        let phi = self.get("phi");
        let root = if bs <= 0. { (phi - bs).sqrt() } else { phi.sqrt() - bs / (2. * phi.sqrt()) };
        let overdrive = gs - (self.get("gamma") * (root - phi.sqrt()) + p * self.get("vto"));
        let cox = if self.get("tox") > 0. { 3.9 * 8.854214871e-12 / self.get("tox") * self.w * (self.l - 2. * self.get("ld")) } else { 0. };
        let saturation = overdrive.max(0.025);
        let (mut cgs, mut cgd, cgb) = if overdrive <= -phi { (0., 0., cox) }
            else if overdrive <= -phi * 0.5 { (0., 0., -overdrive * cox / phi) }
            else if overdrive <= 0. { (2. * overdrive * cox / (1.5 * phi) + 2. * cox / 3., 0., -overdrive * cox / phi) }
            else { (2. * cox / 3., 0., 0.) };
        if overdrive > -phi * 0.5 && ds < saturation {
            let denominator = (2. * saturation - ds).powi(2);
            cgd = cgs * (1. - saturation * saturation / denominator);
            cgs *= 1. - (saturation - ds).powi(2) / denominator;
        }
        let (cgs, cgd) = if source == 2 { (cgs, cgd) } else { (cgd, cgs) };
        vec![(1, 2, cgs), (1, 0, cgd), (1, 3, cgb + self.get("cgbo") * (self.l - 2. * self.get("ld")))]
    }
    pub fn gate_charge(&self, v: [f64; 4], old: [f64; 4], charge: [f64; 4]) -> Option<[Jet; 4]> {
        let caps = self.gate_caps(v);
        if caps.is_empty() { return None; }
        let mut q = charge.map(Jet::constant);
        for ((a, b, cap), (_, _, previous)) in caps.into_iter().zip(self.gate_caps(old)) {
            let delta = Jet::variable(v[a], a) - Jet::variable(v[b], b) - (old[a] - old[b]);
            pair(&mut q, a, b, delta * (0.5 * (cap + previous)));
        }
        Some(q)
    }
    /// Independent current-noise generators, with density in A²/Hz.
    pub fn noise(&self, v: [f64; 4], frequency: f64, temp: f64) -> Vec<(usize, usize, f64)> {
        let eval = self.evaluate(v, v, 0., false);
        let shot = |i: f64| 2. * ELECTRON_CHARGE * i.abs();
        match self.kind {
            b'd' => vec![(0, 1, shot(eval.current[0].v))],
            // SPICE3 uses collector and total base shot currents, also in reverse operation.
            b'q' => vec![(0, 2, shot(eval.current[0].v)), (1, 2, shot(eval.current[1].v))],
            _ => {
                let gm = eval.current[0].d[1].abs();
                let oxide = 3.9 * 8.854214871e-12 / if self.get("tox") > 0. { self.get("tox") } else { 1e-7 };
                let flicker = self.get("kf") * eval.current[0].v.abs().max(1e-38).powf(self.get("af"))
                    / (frequency * self.w * (self.l - 2. * self.get("ld")) * oxide * oxide);
                vec![(0, 2, 8. * BOLTZMANN * (temp + 273.15) * gm / 3. + flicker)]
            }
        }
    }
    fn breakdown(&self) -> f64 { self.get("bv") - self.get("n") * self.vt * (self.get("ibv") / self.get("is")).ln() }
}

fn bandgap(kelvin: f64) -> f64 { 1.16 - 7.02e-4 * kelvin * kelvin / (kelvin + 1108.) }
fn potential_shift(kelvin: f64) -> f64 {
    let vt = VT * kelvin / 300.15;
    bandgap(kelvin) - kelvin / 300.15 * 1.1150877 - 3. * vt * (kelvin / 300.15).ln()
}

pub(super) struct Evaluation { pub voltage: [f64; 4], pub current: [Jet; 4], pub charge: [Jet; 4], pub limited: bool }
fn pair(values: &mut [Jet; 4], a: usize, b: usize, value: Jet) { values[a] = values[a] + value; values[b] = values[b] - value; }

fn depletion(v: Jet, c: f64, potential: f64, grading: f64, fc: f64) -> Jet {
    if c == 0. { return Jet::constant(0.); }
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
