/**
 * Closed-form RF formulas: transmission-line impedance (analysis and synthesis), matching networks,
 * reflection conversions and link budgets. Lengths in mm, frequencies in Hz, impedances in ohms.
 */

export const C0 = 299_792_458;
const ETA0 = 376.730313668;

/** Complete elliptic integral of the first kind, K(k), by the arithmetic-geometric mean. */
export function ellipK(k: number): number {
  let a = 1, b = Math.sqrt(1 - k * k);
  for (let i = 0; i < 40 && Math.abs(a - b) > 1e-15 * a; i++) [a, b] = [(a + b) / 2, Math.sqrt(a * b)];
  return Math.PI / (2 * a);
}

/** K(k)/K(k'), accurate also for k near 0 or 1. */
const kRatio = (k: number) => ellipK(k) / ellipK(Math.sqrt(1 - k * k));

export interface Line {
  /** Characteristic impedance (single-ended), or differential impedance for a pair. */
  z0: number;
  /** Effective permittivity at the given frequency. */
  eeff: number;
  /** Odd- and even-mode impedances of a coupled pair. */
  zOdd?: number;
  zEven?: number;
}

// ---- Microstrip: Hammerstad & Jensen (1980), thickness correction and Kirschning & Jansen dispersion.

const hjZ01 = (u: number) => (ETA0 / (2 * Math.PI)) * Math.log((6 + (2 * Math.PI - 6) * Math.exp(-Math.pow(30.666 / u, 0.7528))) / u + Math.sqrt(1 + 4 / (u * u)));

function hjEeff(u: number, er: number): number {
  const a = 1 + Math.log((u ** 4 + (u / 52) ** 2) / (u ** 4 + 0.432)) / 49 + Math.log(1 + (u / 18.1) ** 3) / 18.7;
  const b = 0.564 * Math.pow((er - 0.9) / (er + 3), 0.053);
  return (er + 1) / 2 + ((er - 1) / 2) * Math.pow(1 + 10 / u, -a * b);
}

/** Static microstrip on a substrate of height h with relative permittivity er; copper thickness t (0 for none). */
function microstripStatic(w: number, h: number, er: number, t: number) {
  const u = w / h;
  let u1 = u, ur = u;
  if (t > 0) {
    const th = t / h;
    const coth = 1 / Math.tanh(Math.sqrt(6.517 * u));
    const du1 = (th / Math.PI) * Math.log(1 + (4 * Math.E) / (th * coth * coth));
    u1 = u + du1;
    ur = u + 0.5 * (1 + 1 / Math.cosh(Math.sqrt(er - 1))) * du1;
  }
  const eeff = hjEeff(ur, er) * (hjZ01(u1) / hjZ01(ur)) ** 2;
  return { z0: hjZ01(ur) / Math.sqrt(hjEeff(ur, er)), eeff, u };
}

/** Kirschning & Jansen (1982) dispersion of microstrip effective permittivity. */
function kjDispersion(u: number, h: number, er: number, eeff0: number, f: number): number {
  if (!f) return eeff0;
  const fn = (f / 1e9) * h; // GHz·mm
  const p1 = 0.27488 + (0.6315 + 0.525 / Math.pow(1 + 0.0157 * fn, 20)) * u - 0.065683 * Math.exp(-8.7513 * u);
  const p2 = 0.33622 * (1 - Math.exp(-0.03442 * er));
  const p3 = 0.0363 * Math.exp(-4.6 * u) * (1 - Math.exp(-Math.pow(fn / 38.7, 4.97)));
  const p4 = 1 + 2.751 * (1 - Math.exp(-Math.pow(er / 15.916, 8)));
  const p = p1 * p2 * Math.pow((0.1844 + p3 * p4) * fn, 1.5763);
  return er - (er - eeff0) / (1 + p);
}

export function microstrip(w: number, h: number, er: number, t = 0, f = 0): Line {
  const s = microstripStatic(w, h, er, t);
  return { z0: s.z0, eeff: kjDispersion(s.u, h, er, s.eeff, f) };
}

// ---- Edge-coupled microstrip pair: Hammerstad & Jensen (1980) even and odd modes (zero thickness).

export function coupledMicrostrip(w: number, gap: number, h: number, er: number, f = 0): Line {
  const u = w / h, g = gap / h;
  const eeff = hjEeff(u, er);
  const z0 = hjZ01(u) / Math.sqrt(eeff);
  // Even mode.
  const v = (u * (20 + g * g)) / (10 + g * g) + g * Math.exp(-g);
  const ae = 1 + Math.log((v ** 4 + (v / 52) ** 2) / (v ** 4 + 0.432)) / 49 + Math.log(1 + (v / 18.1) ** 3) / 18.7;
  const be = 0.564 * Math.pow((er - 0.9) / (er + 3), 0.053);
  const eeffE = (er + 1) / 2 + ((er - 1) / 2) * Math.pow(1 + 10 / v, -ae * be);
  // Odd mode.
  const ao = 0.7287 * (eeff - (er + 1) / 2) * (1 - Math.exp(-0.179 * u));
  const bo = (0.747 * er) / (0.15 + er);
  const co = bo - (bo - 0.207) * Math.exp(-0.414 * u);
  const dO = 0.593 + 0.694 * Math.exp(-0.562 * u);
  const eeffO = ((er + 1) / 2 + ao - eeff) * Math.exp(-co * Math.pow(g, dO)) + eeff;
  const q1 = 0.8695 * Math.pow(u, 0.194);
  const q2 = 1 + 0.7519 * g + 0.189 * Math.pow(g, 2.31);
  const q3 = 0.1975 + Math.pow(16.6 + Math.pow(8.4 / g, 6), -0.387) + Math.log(g ** 10 / (1 + (g / 3.4) ** 10)) / 241;
  const q4 = ((2 * q1) / q2) / (Math.exp(-g) * Math.pow(u, q3) + (2 - Math.exp(-g)) * Math.pow(u, -q3));
  const q5 = 1.794 + 1.14 * Math.log(1 + 0.638 / (g + 0.517 * Math.pow(g, 2.43)));
  const q6 = 0.2305 + Math.log(g ** 10 / (1 + (g / 5.8) ** 10)) / 281.3 + Math.log(1 + 0.598 * Math.pow(g, 1.154)) / 5.1;
  const q7 = (10 + 190 * g * g) / (1 + 82.3 * g ** 3);
  const q8 = Math.exp(-6.5 - 0.95 * Math.log(g) - Math.pow(g / 0.15, 5));
  const q9 = Math.log(q7) * (q8 + 1 / 16.5);
  const q10 = (q2 * q4 - q5 * Math.exp(Math.log(u) * q6 * Math.pow(u, -q9))) / q2;
  const zEven = (z0 * Math.sqrt(eeff / eeffE)) / (1 - (z0 / ETA0) * Math.sqrt(eeff) * q4);
  const zOdd = (z0 * Math.sqrt(eeff / eeffO)) / (1 - (z0 / ETA0) * Math.sqrt(eeff) * q10);
  const eOdd = kjDispersion(u, h, er, eeffO, f);
  return { z0: 2 * zOdd, eeff: eOdd, zOdd, zEven };
}

// ---- Coplanar waveguide with a ground plane below (grounded CPW), conformal mapping (Ghione & Naldi; Wadell 3.4.3).

export function cpwg(w: number, gap: number, h: number, er: number): Line {
  const k = w / (w + 2 * gap);
  const k3 = Math.tanh((Math.PI * w) / (4 * h)) / Math.tanh((Math.PI * (w + 2 * gap)) / (4 * h));
  const r1 = kRatio(k), r3 = kRatio(k3);
  const eeff = (1 + (er * r3) / r1) / (1 + r3 / r1);
  return { z0: (60 * Math.PI) / Math.sqrt(eeff) / (r1 + r3), eeff };
}

// ---- Symmetric stripline between two planes b apart: Wheeler (1978) with strip thickness t.

export function stripline(w: number, b: number, er: number, t = 0): Line {
  if (t <= 0) {
    // Cohn's exact zero-thickness result.
    const k = 1 / Math.cosh((Math.PI * w) / (2 * b));
    return { z0: ((30 * Math.PI) / Math.sqrt(er)) * kRatio(k), eeff: er };
  }
  const x = t / b;
  const n = 2 / (1 + (2 / 3) * (x / (1 - x)));
  const dw = (x / (Math.PI * (1 - x))) * (1 - 0.5 * Math.log((x / (2 - x)) ** 2 + Math.pow((0.0796 * x) / (w / b + 1.1 * x), n)));
  const m = w / (b - t) + dw;
  const a = 8 / (Math.PI * m);
  return { z0: (30 / Math.sqrt(er)) * Math.log(1 + (4 / (Math.PI * m)) * (a + Math.sqrt(a * a + 6.27))), eeff: er };
}

/** Finds the x in [lo, hi] where f(x) = target, for f monotonic in x (bisection on a log scale). */
export function solveFor(f: (x: number) => number, target: number, lo: number, hi: number): number | undefined {
  let flo = f(lo) - target, fhi = f(hi) - target;
  if (flo * fhi > 0) return undefined;
  for (let i = 0; i < 100; i++) {
    const mid = Math.sqrt(lo * hi);
    const fm = f(mid) - target;
    if (Math.abs(fm) < 1e-9) return mid;
    if (fm * flo < 0) [hi, fhi] = [mid, fm];
    else [lo, flo] = [mid, fm];
  }
  return Math.sqrt(lo * hi);
}

// ---- Reflection and wavelength.

export function reflection(zl: { re: number; im: number }, z0 = 50) {
  const nr = zl.re - z0, ni = zl.im, dr = zl.re + z0, di = zl.im;
  const d = dr * dr + di * di;
  const gamma = Math.hypot((nr * dr + ni * di) / d, (ni * dr - nr * di) / d);
  const vswr = gamma >= 1 ? Infinity : (1 + gamma) / (1 - gamma);
  return { gamma, vswr, returnLossDb: -20 * Math.log10(gamma), mismatchLossDb: -10 * Math.log10(1 - gamma * gamma) };
}

export const wavelengthMm = (f: number, eeff = 1) => (C0 / f / Math.sqrt(eeff)) * 1000;

// ---- Matching networks.

export interface Part {
  kind: "L" | "C";
  /** Henries or farads. */
  value: number;
  where: "series" | "shunt";
}

const reactancePart = (x: number, f: number, where: Part["where"]): Part =>
  x >= 0 ? { kind: "L", value: x / (2 * Math.PI * f), where } : { kind: "C", value: -1 / (2 * Math.PI * f * x), where };
const susceptancePart = (b: number, f: number): Part =>
  b >= 0 ? { kind: "C", value: b / (2 * Math.PI * f), where: "shunt" } : { kind: "L", value: -1 / (2 * Math.PI * f * b), where: "shunt" };

/**
 * Two-element L networks that match a load ZL to a real source R0, listed from the source side.
 * Returns every solution (usually two: one low-pass, one high-pass).
 */
export function lMatch(zl: { re: number; im: number }, r0: number, f: number): Part[][] {
  const { re: rl, im: xl } = zl;
  const out: Part[][] = [];
  if (rl <= 0) throw new Error("the load must have a positive resistance");
  {
    // Shunt element next to the load, series element toward the source.
    const disc = (rl / r0) * (rl * rl + xl * xl - r0 * rl);
    if (disc >= 0) {
      for (const sign of [1, -1]) {
        const b = (xl + sign * Math.sqrt(disc)) / (rl * rl + xl * xl);
        const x = 1 / b + (xl * r0) / rl - r0 / (b * rl);
        out.push([reactancePart(x, f, "series"), susceptancePart(b, f)]);
      }
    }
  }
  if (rl < r0) {
    // Series element next to the load, shunt element at the source.
    const root = Math.sqrt(rl * (r0 - rl));
    for (const sign of [1, -1]) {
      const x = sign * root - xl;
      const b = (sign * Math.sqrt((r0 - rl) / rl)) / r0;
      out.push([susceptancePart(b, f), reactancePart(x, f, "series")]);
    }
  }
  // Keep only networks that really match (each formula has a validity region) and drop duplicates.
  const seen = new Set<string>();
  return out.filter((parts) => {
    if (parts.some((p) => !Number.isFinite(p.value) || p.value <= 0)) return false;
    const zin = networkInput(parts, zl, f);
    if (Math.abs(zin.re - r0) > 1e-6 * r0 || Math.abs(zin.im) > 1e-6 * r0) return false;
    const key = parts.map((p) => `${p.where}${p.kind}${p.value.toPrecision(6)}`).join();
    return !seen.has(key) && !!seen.add(key);
  });
}

/** Impedance seen at the source end of a network (listed source side first) terminated in zl. */
export function networkInput(parts: Part[], zl: { re: number; im: number }, f: number): { re: number; im: number } {
  const w = 2 * Math.PI * f;
  let z = { ...zl };
  for (const p of [...parts].reverse()) {
    const x = p.kind === "L" ? w * p.value : -1 / (w * p.value);
    if (p.where === "series") z = { re: z.re, im: z.im + x };
    else {
      // Parallel with jX: z || jX = z·jX / (z + jX).
      const nr = -z.im * x, ni = z.re * x, dr = z.re, di = z.im + x;
      const d = dr * dr + di * di;
      z = { re: (nr * dr + ni * di) / d, im: (ni * dr - nr * di) / d };
    }
  }
  return z;
}

/** Nearest E-series value (E24 for L and C by default) for building a network from real parts. */
export function nearestStandard(value: number, series: number[] = E24): number {
  const decade = Math.pow(10, Math.floor(Math.log10(value)));
  let best = value, err = Infinity;
  for (const d of [decade / 10, decade, decade * 10]) {
    for (const s of series) {
      const v = s * d;
      const e = Math.abs(Math.log(v / value));
      if (e < err) [best, err] = [v, e];
    }
  }
  // Drop the float noise of s * d so 4.7 nH prints as 4.7, not 4.700000000000001.
  return Number(best.toPrecision(6));
}
export const E24 = [1, 1.1, 1.2, 1.3, 1.5, 1.6, 1.8, 2, 2.2, 2.4, 2.7, 3, 3.3, 3.6, 3.9, 4.3, 4.7, 5.1, 5.6, 6.2, 6.8, 7.5, 8.2, 9.1];

// ---- Link budget.

/** Free-space path loss in dB. */
export const fsplDb = (distanceM: number, f: number) => 20 * Math.log10((4 * Math.PI * distanceM * f) / C0);

/**
 * Received power and the range at which the link still has the given fade margin, with a path-loss
 * exponent n (2 = free space, about 2.7–3.5 indoors through walls).
 */
export function linkBudget(o: { txDbm: number; txGainDbi: number; rxGainDbi: number; sensitivityDbm: number; f: number; distanceM?: number; lossDb?: number; marginDb?: number; exponent?: number }) {
  const n = o.exponent ?? 2;
  const loss = o.lossDb ?? 0;
  const margin = o.marginDb ?? 10;
  const at1m = fsplDb(1, o.f);
  const pathLoss = (d: number) => at1m + 10 * n * Math.log10(d);
  const budget = o.txDbm + o.txGainDbi + o.rxGainDbi - loss - o.sensitivityDbm;
  const rangeM = Math.pow(10, (budget - margin - at1m) / (10 * n));
  const rxDbm = o.distanceM === undefined ? undefined : o.txDbm + o.txGainDbi + o.rxGainDbi - loss - pathLoss(o.distanceM);
  return { budgetDb: budget, rangeM, rxDbm, marginAtDistanceDb: rxDbm === undefined ? undefined : rxDbm - o.sensitivityDbm };
}
