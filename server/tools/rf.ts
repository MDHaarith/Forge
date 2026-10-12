import { coupledMicrostrip, cpwg, E24, lMatch, linkBudget, microstrip, nearestStandard, networkInput, reflection, solveFor, stripline, wavelengthMm, type Line, type Part } from "./rfmath.ts";
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
  lines.push(`  Closed-form model (${kind === "cpwg" ? "conformal mapping, copper thickness ignored" : kind === "stripline" ? "Wheeler/Cohn" : "Hammerstad-Jensen"}), typically within 1–2 %; order impedance control and check the fab's own calculator for the final width.`);
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
];
