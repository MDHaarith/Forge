//! Frequency-domain MNA and adjoint noise analysis share the DC solver's dense LU.
use super::{devices::BOLTZMANN, parse::{Circuit, Kind, Noise, Sweep, NONE}, solve::{branch, conductance, current, index, Row, Scalar, System}};
use std::{iter::Sum, ops::{Add, AddAssign, Sub, SubAssign, Mul, Div, DivAssign, Neg}};

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub(super) struct Complex { pub re: f64, pub im: f64 }
impl Complex {
    pub fn polar(magnitude: f64, degrees: f64) -> Self {
        let phase = degrees.to_radians(); Self { re: magnitude * phase.cos(), im: magnitude * phase.sin() }
    }
    pub fn magnitude(self) -> f64 { self.re.hypot(self.im) }
    pub fn phase(self) -> f64 { self.im.atan2(self.re).to_degrees() }
}
impl From<f64> for Complex { fn from(re: f64) -> Self { Self { re, im: 0. } } }
impl Add for Complex { type Output = Self; fn add(self, b: Self) -> Self { Self { re: self.re + b.re, im: self.im + b.im } } }
impl Sub for Complex { type Output = Self; fn sub(self, b: Self) -> Self { self + -b } }
impl Neg for Complex { type Output = Self; fn neg(self) -> Self { Self { re: -self.re, im: -self.im } } }
impl Mul for Complex { type Output = Self; fn mul(self, b: Self) -> Self { Self { re: self.re * b.re - self.im * b.im, im: self.re * b.im + self.im * b.re } } }
impl Div for Complex {
    type Output = Self;
    fn div(self, b: Self) -> Self {
        // Scaling avoids squaring very large or very small matrix pivots.
        if b.re.abs() >= b.im.abs() {
            let ratio = b.im / b.re; let denominator = b.re + b.im * ratio;
            Self { re: (self.re + self.im * ratio) / denominator, im: (self.im - self.re * ratio) / denominator }
        } else {
            let ratio = b.re / b.im; let denominator = b.im + b.re * ratio;
            Self { re: (self.re * ratio + self.im) / denominator, im: (self.im * ratio - self.re) / denominator }
        }
    }
}
impl AddAssign for Complex { fn add_assign(&mut self, b: Self) { *self = *self + b; } }
impl SubAssign for Complex { fn sub_assign(&mut self, b: Self) { *self = *self - b; } }
impl DivAssign for Complex { fn div_assign(&mut self, b: Self) { *self = *self / b; } }
impl Sum for Complex { fn sum<I: Iterator<Item=Self>>(values: I) -> Self { values.fold(Self::default(), |a, b| a + b) } }
impl Scalar for Complex { fn norm(self) -> f64 { self.magnitude() } fn finite(self) -> bool { self.re.is_finite() && self.im.is_finite() } }

fn assemble(c: &Circuit, op: &[f64], frequency: f64, s: &mut System<Complex>, source: Option<usize>) {
    s.reset();
    let omega = std::f64::consts::TAU * frequency;
    for (id, e) in c.elements.iter().enumerate() {
        let [a, b, cp, cn] = e.nodes;
        match &e.kind {
            Kind::R(r) => conductance(s, a, b, (1. / r).into()),
            Kind::C(cap, _) => conductance(s, a, b, Complex { re: 0., im: omega * cap }),
            Kind::L(inductance, _) => { branch(s, e); s.add(e.branch, e.branch, Complex { re: 0., im: -omega * inductance }); }
            Kind::V(v) | Kind::I(v) => {
                let value = if let Some(source) = source { Complex::from(if id == source { 1. } else { 0. }) }
                    else { v.ac.map(|(mag, phase)| Complex::polar(mag, phase)).unwrap_or_default() };
                if matches!(e.kind, Kind::V(_)) { branch(s, e); s.rhs(e.branch, value); }
                else { current(s, a, b, value); }
            }
            Kind::E(gain) => { branch(s, e); s.add(e.branch, index(cp), (-gain).into()); s.add(e.branch, index(cn), (*gain).into()); }
            Kind::G(gain) => {
                s.add(index(a), index(cp), (*gain).into()); s.add(index(a), index(cn), (-gain).into());
                s.add(index(b), index(cp), (-gain).into()); s.add(index(b), index(cn), (*gain).into());
            }
            Kind::Device(model) => {
                let eval = model.small_signal(e.voltages(op), c.options.gmin);
                for i in 0..model.terminal_count() {
                    for j in 0..model.terminal_count() {
                        s.add(index(e.nodes[i]), index(e.nodes[j]), Complex { re: eval.current[i].d[j], im: omega * eval.charge[i].d[j] });
                    }
                }
            }
        }
    }
}
pub(super) struct AcRow { pub frequency: f64, pub x: Vec<Complex> }
pub(super) fn ac(c: &Circuit, op: &[f64], sweep: &Sweep) -> Result<Vec<AcRow>, String> {
    output_limit(c, sweep)?;
    let mut system = System::new(c.size);
    sweep.frequencies.iter().map(|&frequency| {
        assemble(c, op, frequency, &mut system, None);
        let x = system.solve().ok_or_else(|| format!("AC matrix is singular at f={frequency}"))?;
        Ok(AcRow { frequency, x })
    }).collect()
}
pub(super) fn noise(c: &Circuit, op: &[f64], noise: &Noise) -> Result<(Vec<Row>, [f64; 2]), String> {
    let mut system = System::new(c.size);
    let mut output = vec![Complex::default(); c.size];
    let [out, reference] = noise.output.map(index);
    if out != NONE { output[out] += 1f64.into(); }
    if reference != NONE { output[reference] -= 1f64.into(); }
    let mut rows: Vec<Row> = Vec::new();
    let mut totals = [0.; 2];
    let mut previous: Vec<f64> = vec![];
    for &frequency in &noise.sweep.frequencies {
        assemble(c, op, frequency, &mut system, Some(noise.source));
        let x = system.solve().ok_or_else(|| format!("noise matrix is singular at f={frequency}"))?;
        let voltage = |node: usize| if node == NONE { Complex::default() } else { x[node] };
        let gain = (voltage(out) - voltage(reference)).magnitude();
        let adjoint = system.transpose(&output).ok_or("noise adjoint did not converge")?;
        let response = |node: usize| if node == 0 { Complex::default() } else { adjoint[node - 1] };
        let mut contributions = vec![];
        for e in &c.elements {
            match &e.kind {
                Kind::R(r) => contributions.push((response(e.nodes[0]) - response(e.nodes[1])).magnitude().powi(2) * 4. * BOLTZMANN * (c.options.temp + 273.15) / r),
                Kind::Device(model) => for (a, b, noise) in model.noise(e.voltages(op), frequency, c.options.temp) {
                    contributions.push((response(e.nodes[a]) - response(e.nodes[b])).magnitude().powi(2) * noise);
                },
                _ => {}
            }
        }
        if gain == 0. { return Err(format!("noise input gain is zero at f={frequency}")); }
        let density: f64 = contributions.iter().sum();
        let input = density / (gain * gain);
        if !density.is_finite() || !input.is_finite() { return Err(format!("noise spectrum is not finite at f={frequency}")); }
        if let Some(row) = rows.last() {
            // Integrate each independent generator before summing, as SPICE3 does.
            for (a, b) in previous.iter().zip(&contributions) {
                let integrated = integrate(row.axis, frequency, *a, *b);
                totals[0] += integrated;
                // SPICE3 applies the current frequency's input-gain factor at both integration endpoints.
                totals[1] += integrated / (gain * gain);
            }
        }
        previous = contributions;
        rows.push(Row { axis: frequency, x: vec![density.sqrt(), input.sqrt()] });
    }
    Ok((rows, totals.map(f64::sqrt)))
}
fn output_limit(c: &Circuit, sweep: &Sweep) -> Result<(), String> {
    // shortcut: 2 million saved scalars, stream output for larger frequency sweeps.
    if sweep.frequencies.len().saturating_mul(2 * c.size + 1) > 2_000_000 { return Err("simulation output too large (limit 2000000 values)".into()); }
    Ok(())
}
/// Integrate the local power law, including flat thermal and 1/f spectra.
fn integrate(a: f64, b: f64, ya: f64, yb: f64) -> f64 {
    if a == b { return 0.; }
    if ya <= 0. || yb <= 0. { return 0.5 * (ya + yb) * (b - a); }
    let ratio = (b / a).ln();
    let power = (yb / ya).ln() / ratio + 1.;
    if power.abs() < 1e-10 { ya * a * ratio } else { ya * a * (power * ratio).exp_m1() / power }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complex_lu_and_adjoint_pivot_and_reuse_factors() {
        let mut s = System::new(2);
        s.a = vec![0f64.into(), Complex { re: 2., im: 1. }, 1f64.into(), Complex { re: -1., im: 2. }];
        let expected = [Complex { re: 1., im: -1. }, Complex { re: 2., im: 3. }];
        s.b = (0..2).map(|i| (0..2).map(|j| s.a[i * 2 + j] * expected[j]).sum()).collect();
        for _ in 0..2 { for (a, b) in s.solve().unwrap().iter().zip(expected) { assert!((*a - b).magnitude() < 1e-12); } }
        let b: Vec<_> = (0..2).map(|i| (0..2).map(|j| s.a[j * 2 + i] * expected[j]).sum()).collect();
        for (a, b) in s.transpose(&b).unwrap().iter().zip(expected) { assert!((*a - b).magnitude() < 1e-12); }
        assert!((integrate(1., 100., 1., 0.01) - 100f64.ln()).abs() < 1e-12);
    }
}
