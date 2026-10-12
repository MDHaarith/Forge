import { coupledMicrostrip, cpwg, E24, lMatch, linkBudget, microstrip, nearestStandard, networkInput, reflection, solveFor, stripline, wavelengthMm, type Line, type Part } from "./rfmath.ts";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { clip, findProgram, run } from "./exec.ts";
import { projectPath } from "./paths.ts";
import { parseRunnerOutput, RUNNERS } from "./script3d.ts";
import { ToolError, type ForgeTool } from "./types.ts";

/** Dielectric between an outer layer and its reference plane, from the fab's published stackups. */
export const STACKUPS: Record<string, { h: number; er: number; t: number; note: string }> = {
  "jlc-2layer-1.6": { h: 1.53, er: 4.5, t: 0.035, note: "JLCPCB 2-layer 1.6 mm FR-4, top layer over a bottom ground" },
  "jlc-4layer-1.6-7628": { h: 0.2104, er: 4.4, t: 0.035, note: "JLCPCB JLC04161H-7628, outer layer over layer 2 (7628 prepreg)" },
  "jlc-4layer-1.6-3313": { h: 0.0994, er: 4.1, t: 0.035, note: "JLCPCB JLC04161H-3313, outer layer over layer 2 (3313 prepreg)" },
};

const KINDS = ["microstrip", "cpwg", "stripline", "diff_microstrip"] as const;
type Kind = (typeof KINDS)[number];

const num = (v: unknown, what: string, min = -Infinity, max = Infinity): number => {
  const n = Number(v);
  if (v === undefined || v === null || v === "" || !Number.isFinite(n) || n < min || n > max) throw new ToolError(`${what} must be a number from ${min} to ${max}`);
  return n;
};
const opt = (v: unknown, what: string, min?: number, max?: number) => (v === undefined ? undefined : num(v, what, min, max));
const r = (x: number, n = 3) => Number(x.toPrecision(n));
const si = (v: number, unit: "H" | "F") => {
  const [scale, prefix] = unit === "H" ? (v < 1e-6 ? [1e9, "n"] : [1e6, "µ"]) : v < 1e-9 ? [1e12, "p"] : [1e9, "n"];
  return `${r(v * scale, 3)} ${prefix}${unit}`;
};

function line(kind: Kind, w: number, o: { h: number; er: number; t: number; gap?: number; f: number }): Line {
  switch (kind) {
    case "microstrip":
      return microstrip(w, o.h, o.er, o.t, o.f);
    case "cpwg":
      return cpwg(w, o.gap!, o.h, o.er);
    case "stripline":
      return stripline(w, o.h, o.er, o.t);
    case "diff_microstrip":
      return coupledMicrostrip(w, o.gap!, o.h, o.er, o.f);
  }
}

function calcLine(input: Record<string, unknown>): string {
  const kind = String(input.line ?? "microstrip") as Kind;
  if (!KINDS.includes(kind)) throw new ToolError(`line must be one of ${KINDS.join(", ")}`);
  const preset = input.stackup === undefined ? undefined : STACKUPS[String(input.stackup)];
  if (input.stackup !== undefined && !preset) throw new ToolError(`unknown stackup; use one of ${Object.keys(STACKUPS).join(", ")}, or give height_mm and er`);
  const h = opt(input.height_mm, "height_mm", 0.01, 20) ?? preset?.h;
  const er = opt(input.er, "er", 1, 20) ?? preset?.er;
  const t = opt(input.copper_mm, "copper_mm", 0, 0.2) ?? preset?.t ?? 0.035;
  if (h === undefined || er === undefined) throw new ToolError("give a stackup, or height_mm (dielectric to the reference plane; for stripline the plane-to-plane spacing) and er");
  const f = (opt(input.frequency_mhz, "frequency_mhz", 1, 300_000) ?? 2450) * 1e6;
  const gap = opt(input.gap_mm, "gap_mm", 0.01, 50);
  if ((kind === "cpwg" || kind === "diff_microstrip") && gap === undefined) throw new ToolError(`${kind} needs gap_mm (${kind === "cpwg" ? "track to side ground" : "between the two traces"})`);
  const o = { h, er, t, gap, f };

  let w = opt(input.width_mm, "width_mm", 0.01, 50);
  const target = opt(input.target_ohms, "target_ohms", 5, 300);
  if (w === undefined && target === undefined) throw new ToolError("give width_mm to analyse a trace, or target_ohms to find its width");
  const lines: string[] = [];
  if (w === undefined) {
    // Impedance falls as the trace widens.
    const found = solveFor((x) => line(kind, x, o).z0, target!, 0.01, 50);
    if (found === undefined) throw new ToolError(`no width from 0.01 to 50 mm gives ${target} ohm on this stackup${gap !== undefined ? ` with a ${gap} mm gap` : ""}; change the gap or the stackup`);
    w = found;
    lines.push(`Width for ${target} ohm${kind === "diff_microstrip" ? " differential" : ""}: ${r(w, 3)} mm.`);
  }
  const res = line(kind, w, o);
  const lambda = wavelengthMm(f, res.eeff);
  lines.push(
    `${kind} ${r(w, 3)} mm wide${gap !== undefined ? `, ${gap} mm gap` : ""} on ${h} mm, er ${er}${preset ? ` (${preset.note})` : ""}:`,
    `  Z0 ${r(res.z0, 4)} ohm${res.zOdd ? ` differential (odd mode ${r(res.zOdd, 4)}, even mode ${r(res.zEven!, 4)} ohm)` : ""}; effective er ${r(res.eeff, 4)} at ${f / 1e6} MHz`,
    `  wavelength on the line ${r(lambda, 4)} mm; a quarter wave is ${r(lambda / 4, 4)} mm`,
  );
  const wNarrow = 0.1;
  if (w < wNarrow) lines.push(`  ${r(w, 3)} mm is below the 0.1 mm (4 mil) most fabs make reliably; use a thinner dielectric or a wider gap.`);
  lines.push(
    kind === "cpwg"
      ? "  Closed-form model (conformal mapping) that ignores copper thickness: when the gap is under about 5 times the copper thickness the real line reads a few ohms lower. Order impedance control and check the fab's own calculator for the final width."
      : `  Closed-form model (${kind === "stripline" ? "Wheeler/Cohn" : "Hammerstad-Jensen"}), typically within 1–2 %; order impedance control and check the fab's own calculator for the final width.`,
  );
  return lines.join("\n");
}

function describe(parts: Part[]): string {
  return parts.map((p) => `${p.where} ${p.kind} ${si(p.value, p.kind === "L" ? "H" : "F")}`).join(" → ");
}

function calcMatch(input: Record<string, unknown>): string {
  const zl = { re: num(input.load_re, "load_re (load resistance, ohm)", 0.01, 10_000), im: num(input.load_im ?? 0, "load_im (load reactance, ohm)", -10_000, 10_000) };
  const r0 = opt(input.source_ohms, "source_ohms", 1, 1000) ?? 50;
  const f = num(input.frequency_mhz, "frequency_mhz", 0.001, 300_000) * 1e6;
  const before = reflection(zl, r0);
  const nets = lMatch(zl, r0, f);
  if (!nets.length) return `Load ${zl.re}${zl.im < 0 ? "-" : "+"}j${Math.abs(zl.im)} ohm is already matched to ${r0} ohm.`;
  const lines = [`Load ${zl.re}${zl.im < 0 ? " - " : " + "}j${Math.abs(zl.im)} ohm at ${f / 1e6} MHz: return loss ${r(before.returnLossDb, 3)} dB, VSWR ${r(before.vswr, 3)} before matching.`, `L networks to ${r0} ohm, listed from the source (radio) side toward the load (antenna):`];
  nets.forEach((parts, i) => {
    const real = parts.map((p) => ({ ...p, value: nearestStandard(p.value, E24) }));
    const after = reflection(networkInput(real, zl, f), r0);
    lines.push(`  ${i + 1}. ${describe(parts)}`, `     with E24 parts ${describe(real)}: return loss ${r(after.returnLossDb, 3)} dB, VSWR ${r(after.vswr, 3)}`);
  });
  lines.push("Lay the parts out as a pi footprint (shunt-series-shunt, 0402) so either network fits, and fit the final values after measuring the antenna on a VNA: the board, enclosure and hand change its impedance.");
  return lines.join("\n");
}

function calcLink(input: Record<string, unknown>): string {
  const env = String(input.environment ?? "free_space");
  const exponent = opt(input.path_loss_exponent, "path_loss_exponent", 1.5, 6) ?? { free_space: 2, outdoor: 2.7, indoor: 3.3 }[env];
  if (exponent === undefined) throw new ToolError("environment must be free_space, outdoor or indoor (or give path_loss_exponent)");
  const f = num(input.frequency_mhz, "frequency_mhz", 1, 300_000) * 1e6;
  const o = {
    txDbm: num(input.tx_dbm, "tx_dbm", -50, 60),
    txGainDbi: opt(input.tx_gain_dbi, "tx_gain_dbi", -30, 40) ?? 0,
    rxGainDbi: opt(input.rx_gain_dbi, "rx_gain_dbi", -30, 40) ?? 0,
    sensitivityDbm: num(input.sensitivity_dbm, "sensitivity_dbm", -160, 0),
    f,
    distanceM: opt(input.distance_m, "distance_m", 0.01, 1e8),
    lossDb: opt(input.loss_db, "loss_db", 0, 100),
    marginDb: opt(input.margin_db, "margin_db", 0, 60),
    exponent,
  };
  const b = linkBudget(o);
  const lines = [
    `Link at ${f / 1e6} MHz, ${env} (path-loss exponent ${exponent}): budget ${r(b.budgetDb, 4)} dB from ${o.txDbm} dBm out to ${o.sensitivityDbm} dBm sensitivity.`,
    `  Range with a ${o.marginDb ?? 10} dB fade margin: ${b.rangeM >= 1000 ? `${r(b.rangeM / 1000, 3)} km` : `${r(b.rangeM, 3)} m`}`,
  ];
  if (b.rxDbm !== undefined) lines.push(`  At ${o.distanceM} m: received ${r(b.rxDbm, 4)} dBm, ${r(b.marginAtDistanceDb!, 3)} dB above sensitivity`);
  lines.push("  Estimates from a log-distance model; walls, bodies and the enclosure can cost 10–20 dB more. Transmit power and antenna gain must stay within the band's legal limit (EIRP) where the product is sold.");
  return lines.join("\n");
}

/** Python environments with openEMS, from the Fedora setup script's build or a manual one. */
export const OPENEMS_PYTHON = [process.env.FORGE_OPENEMS_PYTHON, path.join(os.homedir(), "opt", "openEMS", "venv", "bin", "python3"), "/opt/openEMS/venv/bin/python3"].filter(
  (p): p is string => !!p,
);
const NAME = /^[A-Za-z0-9_-]{1,64}$/;

interface SimResult {
  antenna: string;
  port: string;
  cells: number;
  seconds: number;
  f0: number;
  span: [number, number];
  resonance: number;
  s11_min_db: number;
  band: [number, number, boolean] | null;
  s11_f0_db: number;
  zin_f0: [number, number];
  zin_res: [number, number];
  directivity_dbi: number | null;
  peak_gain_dbi: number;
  peak_dir: [number, number];
  radiation_efficiency: number;
  total_efficiency: number;
  ground_under: number;
  files: string[];
}

const mhz = (hz: number) => `${r(hz / 1e6, 4)} MHz`;
const ohms = ([re, im]: [number, number]) => `${r(re, 3)} ${im < 0 ? "-" : "+"} j${r(Math.abs(im), 3)} ohm`;
const pct = (x: number) => `${Math.round(x * 100)} %`;

export function describeSim(board: string, res: SimResult, quality: string): string {
  const lines = [
    `Simulated ${res.antenna} on pcb/${board} with openEMS (${quality} mesh, ${res.cells.toLocaleString("en")} cells, ${Math.round(res.seconds)} s), fed ${res.port}; swept ${mhz(res.span[0])} to ${mhz(res.span[1])}.`,
  ];
  if (res.band) {
    const [lo, hi, edge] = res.band;
    lines.push(`  Best match ${r(res.s11_min_db, 3)} dB at ${mhz(res.resonance)}; S11 below -10 dB from ${mhz(lo)} to ${mhz(hi)} (${r((hi - lo) / 1e6, 3)} MHz${edge ? ", runs past the sweep" : ""}).`);
  } else {
    lines.push(`  Best match only ${r(res.s11_min_db, 3)} dB, at ${mhz(res.resonance)}: S11 never reaches -10 dB in the sweep.`);
  }
  lines.push(`  At ${mhz(res.f0)}: S11 ${r(res.s11_f0_db, 3)} dB, input impedance ${ohms(res.zin_f0)}.`);
  lines.push(
    `  Radiation efficiency ${pct(res.radiation_efficiency)}; with the mismatch ${pct(res.total_efficiency)} of the radio's power is radiated. Peak gain ${r(res.peak_gain_dbi, 3)} dBi toward theta ${res.peak_dir[0]}°, phi ${res.peak_dir[1]}°${res.directivity_dbi === null ? "" : ` (directivity ${r(res.directivity_dbi, 3)} dBi)`}.`,
  );
  if (res.ground_under > 0.1) {
    lines.push(
      `  Warning: ${pct(res.ground_under)} of the antenna's trace has ground copper under it or within 2 mm beside it, which turns it into a transmission line rather than an antenna (low, lossy resonance, poor match). Keep every layer's pour out from under and around the radiating arm: put a <keepout> over the antenna area, or place the antenna at a board edge beyond the ground plane, then rebuild and simulate again.`,
    );
  }
  const inBand = res.band && res.band[0] <= res.f0 && res.f0 <= res.band[1];
  if (!inBand) {
    const off = res.resonance / res.f0;
    // A -3 dB dip is already a real resonance for low-profile antennas (inverted-L, IFA) whose radiation resistance is far from 50 ohm.
    if (res.s11_min_db < -3 && res.ground_under <= 0.1 && Math.abs(off - 1) > 0.03) {
      lines.push(`  The antenna resonates ${off > 1 ? "above" : "below"} the target: ${off > 1 ? "lengthen" : "shorten"} its radiating arm by about ${Math.round(Math.abs(off - 1) * 100)} % and simulate again.`);
    }
    lines.push(`  To match it at ${mhz(res.f0)}: rf_calc with calc "match", load_re ${r(res.zin_f0[0], 4)}, load_im ${r(res.zin_f0[1], 4)}, frequency_mhz ${res.f0 / 1e6}.`);
  }
  lines.push(
    "  The model is the antenna's copper, the board substrate, the copper pours and vias; other parts, the enclosure and a hand nearby are not in it and usually pull the resonance down a few percent. Tune the matching network on a VNA before certification.",
  );
  return lines.join("\n");
}

export const rfTools: ForgeTool[] = [
  {
    domain: "pcb",
    name: "rf_calc",
    description: `RF engineering calculations for boards with radios, antennas and fast signals:
- calc "line": impedance of a microstrip, grounded coplanar waveguide (cpwg), stripline or differential microstrip pair, or the trace width for a target impedance (target_ohms; e.g. 50 for an antenna feed, 90 differential for USB, 100 for Ethernet). Use a fab stackup (${Object.keys(STACKUPS).join(", ")}) or height_mm and er. Gives the wavelength and quarter-wave length on the line too.
- calc "match": L matching networks that bring a load impedance (load_re + j load_im, e.g. a measured or simulated antenna) to 50 ohm, with ideal and E24 part values.
- calc "link": link budget and range from transmit power, antenna gains and receiver sensitivity.
Use the trace width it gives in pcb_build (<trace thickness="...mm" />) and keep RF traces short with ground on both sides.`,
    input_schema: {
      type: "object",
      properties: {
        calc: { type: "string", enum: ["line", "match", "link"] },
        line: { type: "string", enum: [...KINDS], description: "line: geometry (default microstrip)" },
        stackup: { type: "string", enum: Object.keys(STACKUPS), description: "line: fab stackup" },
        height_mm: { type: "number", description: "line: dielectric height to the reference plane (stripline: plane-to-plane)" },
        er: { type: "number", description: "line: relative permittivity of the dielectric" },
        copper_mm: { type: "number", description: "line: copper thickness (default 0.035 = 1 oz)" },
        width_mm: { type: "number", description: "line: trace width to analyse" },
        target_ohms: { type: "number", description: "line: impedance to find the width for (differential for diff_microstrip)" },
        gap_mm: { type: "number", description: "line: cpwg gap to side ground, or diff pair spacing" },
        frequency_mhz: { type: "number", description: "Frequency in MHz (line: default 2450)" },
        load_re: { type: "number", description: "match: load resistance in ohm" },
        load_im: { type: "number", description: "match: load reactance in ohm (negative = capacitive)" },
        source_ohms: { type: "number", description: "match: impedance to match to (default 50)" },
        tx_dbm: { type: "number", description: "link: transmit power" },
        tx_gain_dbi: { type: "number" },
        rx_gain_dbi: { type: "number" },
        sensitivity_dbm: { type: "number", description: "link: receiver sensitivity" },
        distance_m: { type: "number", description: "link: distance to evaluate" },
        loss_db: { type: "number", description: "link: extra losses (cables, enclosure)" },
        margin_db: { type: "number", description: "link: fade margin for the range (default 10)" },
        environment: { type: "string", enum: ["free_space", "outdoor", "indoor"] },
        path_loss_exponent: { type: "number", description: "link: overrides environment" },
      },
      required: ["calc"],
    },
    async run(input) {
      const calc = String(input.calc ?? "");
      const text = calc === "line" ? calcLine(input) : calc === "match" ? calcMatch(input) : calc === "link" ? calcLink(input) : undefined;
      if (text === undefined) throw new ToolError('calc must be "line", "match" or "link"');
      return { text };
    },
  },
  {
    domain: "pcb",
    name: "rf_antenna_sim",
    requires: OPENEMS_PYTHON,
    needs: "openEMS",
    install: { url: "scripts/setup-fedora.sh --only rf (builds openEMS from source)" },
    description: `Simulates a board's antenna with openEMS (a full-wave FDTD field solver), from the board pcb_build made. Reports the resonance, S11 and -10 dB bandwidth, the input impedance at the target frequency, radiation efficiency and peak gain, and writes rf/<board>/<antenna>-s11.svg, <antenna>-pattern.svg (gain in three planes), <antenna>.s1p (Touchstone, for a VNA comparison or a circuit simulator) and <antenna>.json.
The board needs ground copper (a copperpour) under the antenna's feed pad. Use it to check an antenna before ordering boards: fix the length if the resonance is off, then feed the impedance to rf_calc match for the matching network. "fast" takes about a minute, "accurate" several.`,
    input_schema: {
      type: "object",
      properties: {
        board: { type: "string", description: "Name of the board built with pcb_build (reads pcb/<board>.circuit.json)" },
        antenna: { type: "string", description: "Antenna's component name, e.g. ANT1 (needed only when the board has several)" },
        frequency_mhz: { type: "number", description: "Target frequency (default 2450)" },
        span_mhz: { type: "number", description: "Width of the sweep around the target (default 80 % of the target)" },
        quality: { type: "string", enum: ["fast", "accurate"], description: "Mesh density (default fast)" },
        er: { type: "number", description: "Substrate permittivity (default 4.5, FR-4)" },
        loss_tangent: { type: "number", description: "Substrate loss tangent (default 0.02, FR-4 at 2.4 GHz)" },
      },
      required: ["board"],
    },
    async run(input, ctx) {
      const board = String(input.board ?? "");
      if (!NAME.test(board)) throw new ToolError("board is the name pcb_build used: letters, digits, - and _");
      const circuit = projectPath(ctx.projectDir, `pcb/${board}.circuit.json`);
      if (!(await fs.stat(circuit).catch(() => null))?.isFile()) throw new ToolError(`pcb/${board}.circuit.json does not exist; build the board with pcb_build first`);
      const antenna = input.antenna === undefined ? "" : String(input.antenna);
      if (antenna && !/^[A-Za-z0-9_]{1,32}$/.test(antenna)) throw new ToolError("antenna is a component name such as ANT1");
      const f0 = (opt(input.frequency_mhz, "frequency_mhz", 50, 30_000) ?? 2450) * 1e6;
      const span = (opt(input.span_mhz, "span_mhz", 1, 30_000) ?? (0.8 * f0) / 1e6) * 1e6;
      if (span >= 2 * f0) throw new ToolError("span_mhz must be less than twice the frequency");
      const quality = String(input.quality ?? "fast");
      if (quality !== "fast" && quality !== "accurate") throw new ToolError('quality must be "fast" or "accurate"');
      const er = opt(input.er, "er", 1, 20) ?? 4.5;
      const tand = opt(input.loss_tangent, "loss_tangent", 0, 0.2) ?? 0.02;
      const python = findProgram(OPENEMS_PYTHON);
      if (!python) throw new ToolError("openEMS is not installed; run scripts/setup-fedora.sh --only rf");

      const outRel = `rf/${board}`;
      const out = projectPath(ctx.projectDir, outRel);
      const sim = await fs.mkdtemp(path.join(os.tmpdir(), "forge-openems-"));
      const args = { circuit, out, antenna, f0_hz: f0, span_hz: span, quality, er, tand, threads: 0, sim_dir: path.join(sim, "run") };
      const started = Date.now();
      try {
        const res = await run(python, [path.join(RUNNERS, "openems_antenna.py"), JSON.stringify(args)], ctx.projectDir, (quality === "accurate" ? 60 : 20) * 60_000);
        const parsed = parseRunnerOutput(res);
        if (parsed.error) throw new ToolError(clip(parsed.error, 4000));
        const result = { ...(parsed.result as unknown as SimResult), seconds: (Date.now() - started) / 1000 };
        return { text: describeSim(board, result, quality), files: result.files.map((f) => `${outRel}/${f}`) };
      } finally {
        await fs.rm(sim, { recursive: true, force: true });
      }
    },
  },
];
