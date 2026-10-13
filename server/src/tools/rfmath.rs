//! Closed-form RF formulas: transmission-line impedance (analysis and synthesis), matching networks,
//! reflection conversions and link budgets. Lengths in mm, frequencies in Hz, impedances in ohms.

use crate::jsfmt::{precision, precision_string};
use std::{collections::HashSet, f64::consts::{E, PI}};

pub const C0: f64 = 299_792_458.0;
const ETA0: f64 = 376.730313668;

/// Complete elliptic integral of the first kind, K(k), by the arithmetic-geometric mean.
pub fn ellip_k(k: f64) -> f64 {
    let (mut a, mut b) = (1.0, (1.0 - k * k).sqrt());
    let mut i = 0;
    while i < 40 && (a - b).abs() > 1e-15 * a {
        (a, b) = ((a + b) / 2.0, (a * b).sqrt());
        i += 1;
    }
    PI / (2.0 * a)
}

/// K(k)/K(k'), accurate also for k near 0 or 1.
fn k_ratio(k: f64) -> f64 { ellip_k(k) / ellip_k((1.0 - k * k).sqrt()) }

#[derive(Debug)]
pub struct Line {
    /// Characteristic impedance (single-ended), or differential impedance for a pair.
    pub z0: f64,
    /// Effective permittivity at the given frequency.
    pub eeff: f64,
    /// Odd- and even-mode impedances of a coupled pair.
    pub z_odd: Option<f64>,
    pub z_even: Option<f64>,
}
impl Line {
    fn single(z0: f64, eeff: f64) -> Self { Self { z0, eeff, z_odd: None, z_even: None } }
}

// ---- Microstrip: Hammerstad & Jensen (1980), thickness correction and Kirschning & Jansen dispersion.
fn hj_z01(u: f64) -> f64 {
    (ETA0 / (2.0 * PI)) * ((6.0 + (2.0 * PI - 6.0) * (-(30.666 / u).powf(0.7528)).exp()) / u + (1.0 + 4.0 / (u * u)).sqrt()).ln()
}
fn hj_eeff(u: f64, er: f64) -> f64 {
    let a = 1.0 + ((u.powi(4) + (u / 52.0).powi(2)) / (u.powi(4) + 0.432)).ln() / 49.0 + (1.0 + (u / 18.1).powi(3)).ln() / 18.7;
    let b = 0.564 * ((er - 0.9) / (er + 3.0)).powf(0.053);
    (er + 1.0) / 2.0 + ((er - 1.0) / 2.0) * (1.0 + 10.0 / u).powf(-a * b)
}

/// Static microstrip on a substrate of height h with relative permittivity er; copper thickness t (0 for none).
fn microstrip_static(w: f64, h: f64, er: f64, t: f64) -> (f64, f64, f64) {
    let u = w / h;
    let (mut u1, mut ur) = (u, u);
    if t > 0.0 {
        let th = t / h;
        let coth = 1.0 / (6.517 * u).sqrt().tanh();
        let du1 = (th / PI) * (1.0 + (4.0 * E) / (th * coth * coth)).ln();
        u1 = u + du1;
        ur = u + 0.5 * (1.0 + 1.0 / (er - 1.0).sqrt().cosh()) * du1;
    }
    let eeff = hj_eeff(ur, er) * (hj_z01(u1) / hj_z01(ur)).powi(2);
    (hj_z01(ur) / hj_eeff(ur, er).sqrt(), eeff, u)
}

/// Kirschning & Jansen (1982) dispersion of microstrip effective permittivity.
fn kj_dispersion(u: f64, h: f64, er: f64, eeff0: f64, f: f64) -> f64 {
    if f == 0.0 || f.is_nan() { return eeff0; }
    let fnorm = (f / 1e9) * h; // GHz·mm
    let p1 = 0.27488 + (0.6315 + 0.525 / (1.0 + 0.0157 * fnorm).powf(20.0)) * u - 0.065683 * (-8.7513 * u).exp();
    let p2 = 0.33622 * (1.0 - (-0.03442 * er).exp());
    let p3 = 0.0363 * (-4.6 * u).exp() * (1.0 - (-(fnorm / 38.7).powf(4.97)).exp());
    let p4 = 1.0 + 2.751 * (1.0 - (-(er / 15.916).powf(8.0)).exp());
    let p = p1 * p2 * ((0.1844 + p3 * p4) * fnorm).powf(1.5763);
    er - (er - eeff0) / (1.0 + p)
}

pub fn microstrip(w: f64, h: f64, er: f64, t: f64, f: f64) -> Line {
    let (z0, eeff, u) = microstrip_static(w, h, er, t);
    Line::single(z0, kj_dispersion(u, h, er, eeff, f))
}

// ---- Edge-coupled microstrip pair: Hammerstad & Jensen (1980) even and odd modes (zero thickness).
pub fn coupled_microstrip(w: f64, gap: f64, h: f64, er: f64, f: f64) -> Line {
    let (u, g) = (w / h, gap / h);
    let eeff = hj_eeff(u, er);
    let z0 = hj_z01(u) / eeff.sqrt();
    // Even mode.
    let v = (u * (20.0 + g * g)) / (10.0 + g * g) + g * (-g).exp();
    let ae = 1.0 + ((v.powi(4) + (v / 52.0).powi(2)) / (v.powi(4) + 0.432)).ln() / 49.0 + (1.0 + (v / 18.1).powi(3)).ln() / 18.7;
    let be = 0.564 * ((er - 0.9) / (er + 3.0)).powf(0.053);
    let eeff_e = (er + 1.0) / 2.0 + ((er - 1.0) / 2.0) * (1.0 + 10.0 / v).powf(-ae * be);
    // Odd mode.
    let ao = 0.7287 * (eeff - (er + 1.0) / 2.0) * (1.0 - (-0.179 * u).exp());
    let bo = (0.747 * er) / (0.15 + er);
    let co = bo - (bo - 0.207) * (-0.414 * u).exp();
    let d_o = 0.593 + 0.694 * (-0.562 * u).exp();
    let eeff_o = ((er + 1.0) / 2.0 + ao - eeff) * (-co * g.powf(d_o)).exp() + eeff;
    let q1 = 0.8695 * u.powf(0.194);
    let q2 = 1.0 + 0.7519 * g + 0.189 * g.powf(2.31);
    let q3 = 0.1975 + (16.6 + (8.4 / g).powf(6.0)).powf(-0.387) + (g.powi(10) / (1.0 + (g / 3.4).powi(10))).ln() / 241.0;
    let q4 = ((2.0 * q1) / q2) / ((-g).exp() * u.powf(q3) + (2.0 - (-g).exp()) * u.powf(-q3));
    let q5 = 1.794 + 1.14 * (1.0 + 0.638 / (g + 0.517 * g.powf(2.43))).ln();
    let q6 = 0.2305 + (g.powi(10) / (1.0 + (g / 5.8).powi(10))).ln() / 281.3 + (1.0 + 0.598 * g.powf(1.154)).ln() / 5.1;
    let q7 = (10.0 + 190.0 * g * g) / (1.0 + 82.3 * g.powi(3));
    let q8 = (-6.5 - 0.95 * g.ln() - (g / 0.15).powf(5.0)).exp();
    let q9 = q7.ln() * (q8 + 1.0 / 16.5);
    let q10 = (q2 * q4 - q5 * (u.ln() * q6 * u.powf(-q9)).exp()) / q2;
    let z_even = (z0 * (eeff / eeff_e).sqrt()) / (1.0 - (z0 / ETA0) * eeff.sqrt() * q4);
    let z_odd = (z0 * (eeff / eeff_o).sqrt()) / (1.0 - (z0 / ETA0) * eeff.sqrt() * q10);
    Line { z0: 2.0 * z_odd, eeff: kj_dispersion(u, h, er, eeff_o, f), z_odd: Some(z_odd), z_even: Some(z_even) }
}

// ---- Coplanar waveguide with a ground plane below (grounded CPW), conformal mapping (Ghione & Naldi; Wadell 3.4.3).
pub fn cpwg(w: f64, gap: f64, h: f64, er: f64) -> Line {
    let k = w / (w + 2.0 * gap);
    let k3 = ((PI * w) / (4.0 * h)).tanh() / ((PI * (w + 2.0 * gap)) / (4.0 * h)).tanh();
    let (r1, r3) = (k_ratio(k), k_ratio(k3));
    let eeff = (1.0 + (er * r3) / r1) / (1.0 + r3 / r1);
    Line::single((60.0 * PI) / eeff.sqrt() / (r1 + r3), eeff)
}

// ---- Symmetric stripline between two planes b apart: Wheeler (1978) with strip thickness t.
pub fn stripline(w: f64, b: f64, er: f64, t: f64) -> Line {
    if t <= 0.0 {
        // Cohn's exact zero-thickness result.
        let k = 1.0 / ((PI * w) / (2.0 * b)).cosh();
        return Line::single(((30.0 * PI) / er.sqrt()) * k_ratio(k), er);
    }
    let x = t / b;
    let n = 2.0 / (1.0 + (2.0 / 3.0) * (x / (1.0 - x)));
    let dw = (x / (PI * (1.0 - x))) * (1.0 - 0.5 * ((x / (2.0 - x)).powi(2) + ((0.0796 * x) / (w / b + 1.1 * x)).powf(n)).ln());
    let m = w / (b - t) + dw;
    let a = 8.0 / (PI * m);
    Line::single((30.0 / er.sqrt()) * (1.0 + (4.0 / (PI * m)) * (a + (a * a + 6.27).sqrt())).ln(), er)
}

/// Finds the x in [lo, hi] where f(x) = target, for f monotonic in x (bisection on a log scale).
pub fn solve_for(f: impl Fn(f64) -> f64, target: f64, mut lo: f64, mut hi: f64) -> Option<f64> {
    let mut flo = f(lo) - target;
    let fhi = f(hi) - target;
    if flo * fhi > 0.0 { return None; }
    for _ in 0..100 {
        let mid = (lo * hi).sqrt();
        let fm = f(mid) - target;
        if fm.abs() < 1e-9 { return Some(mid); }
        if fm * flo < 0.0 { hi = mid; }
        else { lo = mid; flo = fm; }
    }
    Some((lo * hi).sqrt())
}

// ---- Reflection and wavelength.
#[derive(Clone, Copy, Debug)]
pub struct Impedance { pub re: f64, pub im: f64 }
pub struct Reflection { pub gamma: f64, pub vswr: f64, pub return_loss_db: f64, pub mismatch_loss_db: f64 }
pub fn reflection(zl: Impedance, z0: f64) -> Reflection {
    let (nr, ni, dr, di) = (zl.re - z0, zl.im, zl.re + z0, zl.im);
    let d = dr * dr + di * di;
    let gamma = ((nr * dr + ni * di) / d).hypot((ni * dr - nr * di) / d);
    let vswr = if gamma >= 1.0 { f64::INFINITY } else { (1.0 + gamma) / (1.0 - gamma) };
    Reflection { gamma, vswr, return_loss_db: -20.0 * gamma.log10(), mismatch_loss_db: -10.0 * (1.0 - gamma * gamma).log10() }
}
pub fn wavelength_mm(f: f64, eeff: f64) -> f64 { (C0 / f / eeff.sqrt()) * 1000.0 }

// ---- Matching networks.
#[derive(Clone, Debug)]
pub struct Part {
    pub kind: &'static str,
    /// Henries or farads.
    pub value: f64,
    pub r#where: &'static str,
}
fn reactance_part(x: f64, f: f64, r#where: &'static str) -> Part {
    if x >= 0.0 { Part { kind: "L", value: x / (2.0 * PI * f), r#where } }
    else { Part { kind: "C", value: -1.0 / (2.0 * PI * f * x), r#where } }
}
fn susceptance_part(b: f64, f: f64) -> Part {
    if b >= 0.0 { Part { kind: "C", value: b / (2.0 * PI * f), r#where: "shunt" } }
    else { Part { kind: "L", value: -1.0 / (2.0 * PI * f * b), r#where: "shunt" } }
}

/// Two-element L networks that match a load ZL to a real source R0, listed from the source side.
/// Returns every solution (usually two: one low-pass, one high-pass).
pub fn l_match(zl: Impedance, r0: f64, f: f64) -> Result<Vec<Vec<Part>>, &'static str> {
    let (rl, xl) = (zl.re, zl.im);
    let mut out = Vec::new();
    if rl <= 0.0 { return Err("the load must have a positive resistance"); }
    // Shunt element next to the load, series element toward the source.
    let disc = (rl / r0) * (rl * rl + xl * xl - r0 * rl);
    if disc >= 0.0 {
        for sign in [1.0, -1.0] {
            let b = (xl + sign * disc.sqrt()) / (rl * rl + xl * xl);
            let x = 1.0 / b + (xl * r0) / rl - r0 / (b * rl);
            out.push(vec![reactance_part(x, f, "series"), susceptance_part(b, f)]);
        }
    }
    if rl < r0 {
        // Series element next to the load, shunt element at the source.
        let root = (rl * (r0 - rl)).sqrt();
        for sign in [1.0, -1.0] {
            let x = sign * root - xl;
            let b = (sign * ((r0 - rl) / rl).sqrt()) / r0;
            out.push(vec![susceptance_part(b, f), reactance_part(x, f, "series")]);
        }
    }
    // Keep only networks that really match (each formula has a validity region) and drop duplicates.
    let mut seen = HashSet::new();
    out.retain(|parts| {
        if parts.iter().any(|p| !p.value.is_finite() || p.value <= 0.0) { return false; }
        let zin = network_input(parts, zl, f);
        if (zin.re - r0).abs() > 1e-6 * r0 || zin.im.abs() > 1e-6 * r0 { return false; }
        let key = parts.iter().map(|p| format!("{}{}{}", p.r#where, p.kind, precision_string(p.value, 6))).collect::<Vec<_>>().join(",");
        seen.insert(key)
    });
    Ok(out)
}

/// Impedance seen at the source end of a network (listed source side first) terminated in zl.
pub fn network_input(parts: &[Part], zl: Impedance, f: f64) -> Impedance {
    let w = 2.0 * PI * f;
    let mut z = zl;
    for p in parts.iter().rev() {
        let x = if p.kind == "L" { w * p.value } else { -1.0 / (w * p.value) };
        if p.r#where == "series" { z.im += x; }
        else {
            // Parallel with jX: z || jX = z·jX / (z + jX).
            let (nr, ni, dr, di) = (-z.im * x, z.re * x, z.re, z.im + x);
            let d = dr * dr + di * di;
            z = Impedance { re: (nr * dr + ni * di) / d, im: (ni * dr - nr * di) / d };
        }
    }
    z
}

/// Nearest E-series value (E24 for L and C by default) for building a network from real parts.
pub fn nearest_standard(value: f64, series: &[f64]) -> f64 {
    let decade = 10f64.powf(value.log10().floor());
    let (mut best, mut err) = (value, f64::INFINITY);
    for d in [decade / 10.0, decade, decade * 10.0] {
        for s in series {
            let v = s * d;
            let e = (v / value).ln().abs();
            if e < err { best = v; err = e; }
        }
    }
    // Drop the float noise of s * d so 4.7 nH prints as 4.7, not 4.700000000000001.
    precision(best, 6)
}
pub const E24: &[f64] = &[1.0, 1.1, 1.2, 1.3, 1.5, 1.6, 1.8, 2.0, 2.2, 2.4, 2.7, 3.0, 3.3, 3.6, 3.9, 4.3, 4.7, 5.1, 5.6, 6.2, 6.8, 7.5, 8.2, 9.1];

// ---- Link budget.
/// Free-space path loss in dB.
pub fn fspl_db(distance_m: f64, f: f64) -> f64 { 20.0 * ((4.0 * PI * distance_m * f) / C0).log10() }

pub struct LinkOptions {
    pub tx_dbm: f64, pub tx_gain_dbi: f64, pub rx_gain_dbi: f64, pub sensitivity_dbm: f64, pub f: f64,
    pub distance_m: Option<f64>, pub loss_db: Option<f64>, pub margin_db: Option<f64>, pub exponent: Option<f64>,
}
pub struct LinkBudget { pub budget_db: f64, pub range_m: f64, pub rx_dbm: Option<f64>, pub margin_at_distance_db: Option<f64> }
/// Received power and the range at which the link still has the given fade margin, with a path-loss
/// exponent n (2 = free space, about 2.7–3.5 indoors through walls).
pub fn link_budget(o: &LinkOptions) -> LinkBudget {
    let n = o.exponent.unwrap_or(2.0);
    let loss = o.loss_db.unwrap_or(0.0);
    let margin = o.margin_db.unwrap_or(10.0);
    let at1m = fspl_db(1.0, o.f);
    let path_loss = |d: f64| at1m + 10.0 * n * d.log10();
    let budget = o.tx_dbm + o.tx_gain_dbi + o.rx_gain_dbi - loss - o.sensitivity_dbm;
    let range_m = 10f64.powf((budget - margin - at1m) / (10.0 * n));
    let rx_dbm = o.distance_m.map(|d| o.tx_dbm + o.tx_gain_dbi + o.rx_gain_dbi - loss - path_loss(d));
    LinkBudget { budget_db: budget, range_m, rx_dbm, margin_at_distance_db: rx_dbm.map(|rx| rx - o.sensitivity_dbm) }
}

// IPC-2221 copper area is in square mils; all public dimensions are mm.
pub fn track_width(current: f64, rise: f64, copper: f64, internal: bool) -> f64 {
    let k: f64 = if internal { 0.024 } else { 0.048 };
    (current / (k * rise.powf(0.44))).powf(1. / 0.725) * 0.0254_f64.powi(2) / copper
}
pub fn via_properties(drill: f64, length: f64, plating: f64, rise: f64) -> (f64, f64) {
    let area = PI * (drill + plating) * plating;
    (1.72e-5 * length / area, 0.048 * rise.powf(0.44) * (area / 0.0254_f64.powi(2)).powf(0.725))
}
// Onderdonk's adiabatic copper model, area in circular mils, ambient 20 C and melting point 1083 C.
pub fn fusing_current(width: f64, copper: f64, seconds: f64) -> f64 {
    let circular_mils = width * copper / (PI / 4. * 0.0254_f64.powi(2));
    circular_mils * ((1. + 1063_f64 / 254.).log10() / (33. * seconds)).sqrt()
}

pub const E12: &[f64] = &[1., 1.2, 1.5, 1.8, 2.2, 2.7, 3.3, 3.9, 4.7, 5.6, 6.8, 8.2];
pub const E96: &[f64] = &[
    1.00,1.02,1.05,1.07,1.10,1.13,1.15,1.18,1.21,1.24,1.27,1.30,
    1.33,1.37,1.40,1.43,1.47,1.50,1.54,1.58,1.62,1.65,1.69,1.74,
    1.78,1.82,1.87,1.91,1.96,2.00,2.05,2.10,2.15,2.21,2.26,2.32,
    2.37,2.43,2.49,2.55,2.61,2.67,2.74,2.80,2.87,2.94,3.01,3.09,
    3.16,3.24,3.32,3.40,3.48,3.57,3.65,3.74,3.83,3.92,4.02,4.12,
    4.22,4.32,4.42,4.53,4.64,4.75,4.87,4.99,5.11,5.23,5.36,5.49,
    5.62,5.76,5.90,6.04,6.19,6.34,6.49,6.65,6.81,6.98,7.15,7.32,
    7.50,7.68,7.87,8.06,8.25,8.45,8.66,8.87,9.09,9.31,9.53,9.76,
];

/// Find the closest unloaded divider ratio; fixing one resistor decade removes equivalent scaled pairs.
pub fn divider(vin: f64, target: f64, series: &[f64]) -> (f64, f64, f64) {
    let mut best = (0.,0.,0.);
    let mut error = f64::INFINITY;
    for value in series {
        let bottom = value * 10_000.;
        let ideal = bottom * (vin - target) / target;
        let decade = 10_f64.powf(ideal.log10().floor());
        for d in [decade / 10.,decade,decade * 10.] {
            for v in series {
                let top = v * d;
                let output = vin * bottom / (top + bottom);
                let e = (output - target).abs();
                if e < error { error = e; best = (top,bottom,output); }
            }
        }
    }
    best
}

/// IPC-2221B table 6-1, B1/B2/B4. Above 500 V add each column's per-volt increment.
pub fn conductor_spacing(voltage: f64, column: usize) -> f64 {
    const LIMITS: [f64;7] = [15.,30.,50.,100.,150.,300.,500.];
    const SPACING: [[f64;3];7] = [
        [0.05,0.1,0.05],[0.05,0.1,0.05],[0.1,0.6,0.13],[0.1,0.6,0.13],
        [0.2,0.6,0.4],[0.2,1.25,0.4],[0.25,2.5,0.8],
    ];
    let row = LIMITS.iter().position(|&v| voltage <= v).unwrap_or(6);
    SPACING[row][column] + (voltage-500.).max(0.) * [0.0025,0.005,0.00305][column]
}

#[cfg(test)]
mod pcb_calculator_tests {
    use super::*;
    #[test]
    fn copper_models_and_spacing_boundaries() {
        let external = track_width(1.,10.,0.035,false);
        assert!((external-0.300).abs() < 0.001);
        assert!((track_width(0.5,10.,0.035,true)-external).abs() < 1e-12);
        let (resistance,current) = via_properties(0.3,1.6,0.018,10.);
        assert!((resistance*1000.-1.53).abs() < 0.01);
        assert!(current > 1.);
        assert!((fusing_current(0.5,0.035,1.)-5.08).abs() < 0.01);
        assert!((fusing_current(0.5,0.035,4.)*2.-fusing_current(0.5,0.035,1.)).abs() < 1e-12);
        assert_eq!(conductor_spacing(30.,1),0.1);
        assert_eq!(conductor_spacing(30.001,1),0.6);
        assert_eq!(conductor_spacing(250.,2),0.4);
        assert_eq!(conductor_spacing(600.,0),0.5);
    }
    #[test]
    fn standard_values_and_divider_ratios() {
        assert_eq!(nearest_standard(5000.,E96),4990.);
        assert_eq!(nearest_standard(9.9,E12),10.);
        for series in [E12,E24,E96] {
            let (top,bottom,output) = divider(5.,3.3,series);
            assert!(top > 0. && bottom > 0.);
            assert_eq!(output,5.*bottom/(top+bottom));
            assert!((output-3.3).abs()/3.3 < if series == E12 { 0.03 } else { 0.01 });
        }
    }
}
