import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { clip } from "./exec.ts";
import { projectPath } from "./paths.ts";
import { parseRunnerOutput } from "./script3d.ts";
import { ToolError, type ForgeTool } from "./types.ts";

const NAME = /^[A-Za-z0-9_-]{1,64}$/;
const ENGINE_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..", "engines", "pcb");

/** The built-in PCB engine, bundled into one file by `npm run pcb-engine` (the desktop app ships it). */
export function pcbEngine(dir = ENGINE_DIR): string | undefined {
  const bundle = path.join(dir, "dist", "pcb-engine.mjs");
  return existsSync(bundle) ? bundle : undefined;
}

// Warnings every quick board has and that say nothing about whether it works.
const QUIET = new Set(["source_unnamed_trace_warning", "schematic_missing_sheet_warning"]);

/** Groups the engine's warnings by kind, so 30 identical "no part number" lines read as one. */
export function summarizeWarnings(warnings: string[], lookedUp: boolean): string[] {
  const groups = new Map<string, string[]>();
  for (const w of warnings) {
    const kind = w.split(":")[0];
    if (QUIET.has(kind)) continue;
    groups.set(kind, [...(groups.get(kind) ?? []), w.slice(kind.length + 1).trim()]);
  }
  const lines: string[] = [];
  for (const [kind, messages] of groups) {
    if (kind === "source_part_not_found_warning" && !lookedUp) continue;
    if (kind === "source_missing_manufacturer_part_number_warning") {
      lines.push(`${messages.length} part(s) have no manufacturer part number, so the BOM has no LCSC numbers${lookedUp ? "" : " (part lookup is off; pass lookup_parts: true to look them up on JLCPCB, or set supplierPartNumbers)"}`);
      continue;
    }
    lines.push(messages.length === 1 ? `${kind}: ${messages[0]}` : `${kind} ×${messages.length}, e.g. ${messages[0]}`);
  }
  return lines;
}

/** Runs the engine in a child Node that may only read the engine and the project, write the project, and start nothing. */
function runEngine(engine: string, projectDir: string, args: Record<string, unknown>, timeoutMs: number) {
  const node = [
    "--permission",
    `--allow-fs-read=${engine}`,
    `--allow-fs-read=${projectDir}`,
    `--allow-fs-write=${projectDir}`,
    "--disable-warning=ExperimentalWarning",
    "--disable-warning=SecurityWarning",
    engine,
  ];
  // Only what the engine needs: the board code must not see Forge's API key.
  const env = { PATH: process.env.PATH ?? "", LANG: process.env.LANG ?? "C.UTF-8", FORGE_ARGS: JSON.stringify(args) };
  return new Promise<{ code: number; stdout: string; stderr: string }>((resolve) => {
    execFile(process.execPath, node, { cwd: projectDir, env, timeout: timeoutMs, maxBuffer: 32 * 1024 * 1024 }, (error, stdout, stderr) => {
      const timedOut = error && (error as { killed?: boolean }).killed;
      resolve({
        code: error ? (typeof error.code === "number" ? error.code : 1) : 0,
        stdout: String(stdout),
        stderr: timedOut ? `the board took longer than ${timeoutMs / 1000} s to build (a large autoroute?)` : String(stderr),
      });
    });
  });
}

export const boardTools: ForgeTool[] = [
  {
    domain: "pcb",
    name: "pcb_build",
    description: `Build a printed circuit board from tscircuit code with Forge's built-in PCB engine (no KiCad needed): places parts, autoroutes the traces, and writes the PCB view, schematic, 3D model and fab files. The board then shows in the Design view next to the 3D assembly.
Write the board first with write_file as pcb/<name>.tsx: a default-exported React component returning one <board width="50mm" height="30mm"> (sizes in mm; the board is centred on 0,0). Inside it:
  <resistor name="R1" resistance="10k" footprint="0402" pcbX={-5} pcbY={2} /> · <capacitor name="C1" capacitance="100nF" footprint="0603" /> · <led name="LED1" color="red" footprint="0805" /> · <diode> · <inductor> · <crystal> · <pushbutton> · <switch>
  <chip name="U1" footprint="soic8" pinLabels={{ pin1: "VCC", pin8: "GND" }} /> for ICs (footprints: soic8, sot23, sot23_5, qfn20, tssop16, dip8, bga64 ...) and <pinheader name="J1" pinCount={4} pitch="2.54mm" />, <hole diameter="3.2mm" pcbX={..} pcbY={..} /> for mounting holes
  <trace from=".R1 > .pin2" to=".LED1 > .anode" /> connects ports; <trace from=".U1 > .VCC" to="net.VCC" /> joins a named net; <net name="GND" /> declares one
  pcbX/pcbY place a part in mm, pcbRotation turns it, layer="bottom" puts it underneath. A <board> also takes outline={[{x,y},...]} for a custom shape, layers={4} and thickness="1.6mm".
  Ground and RF: <copperpour connectsTo="net.GND" layer="bottom" /> pours a ground plane (one per layer); <keepout pcbX={..} pcbY={..} width="10mm" height="6mm" shape="rect" /> keeps copper out; <trace ... thickness="0.35mm" /> sets a trace width (use rf_calc for a 50 ohm width); <differentialpair positiveConnection=".U1 > .DP" negativeConnection=".U1 > .DN" targetDifferentialImpedance="90ohm" /> pairs USB/Ethernet lines.
  Antennas: <antenna name="ANT1" antennaShape="2.4ghz_inverted_f" /> generates a printed 2.4 GHz antenna with its own keepout (shapes: 2.4ghz_quarter_wave_monopole, 2.4ghz_meandered_monopole, 2.4ghz_inverted_f, 2.4ghz_meandered_inverted_f, 2.4ghz_folded_dipole); .pin1 is the feed; .pin2 is the ground return, a plated hole that joins a bottom ground pour (connect it to net.GND). Place it on a board edge with no ground copper under or beside it, and feed it through a pi matching network (three 0402 footprints) so it can be tuned after measurement. For a first product prefer a pre-certified radio module (e.g. ESP32-C3-MINI) whose antenna and certification are already done.
Other .tsx files in the same folder can be imported for reusable blocks. The tool reports the board size, parts, traces, vias and every error (unroutable traces, overlapping parts, bad connections): fix errors and rebuild before calling the board done.
Outputs: pcb/<name>-pcb.svg, pcb/<name>-schematic.svg, pcb/<name>.glb (3D), pcb/<name>.circuit.json, pcb/<name>.board.json (part boxes for design_assemble) and fab/<name>/ with Gerbers, PTH/NPTH drill files, pick-and-place and a BOM in the format JLCPCB accepts.`,
    input_schema: {
      type: "object",
      properties: {
        source: { type: "string", description: "Project-relative .tsx board file, e.g. pcb/main.tsx" },
        name: { type: "string", description: "Base name for the outputs (default: the source file's name)" },
        lookup_parts: { type: "boolean", description: "Look up JLCPCB part numbers for the BOM online (default false: the build stays offline)" },
      },
      required: ["source"],
    },
    requires: path.join(ENGINE_DIR, "dist", "pcb-engine.mjs"),
    needs: "Forge's PCB engine",
    install: { url: "npm run pcb-engine" },
    async run(input, ctx) {
      const source = String(input.source ?? "");
      const full = projectPath(ctx.projectDir, source);
      if (!/\.(tsx|jsx)$/.test(source)) throw new ToolError("source must be a .tsx file");
      if (!(await fs.stat(full).catch(() => null))?.isFile()) throw new ToolError(`${source} does not exist; write it with write_file first`);
      const name = input.name === undefined ? path.basename(source).replace(/\.[^.]+$/, "") : String(input.name);
      if (!NAME.test(name)) throw new ToolError("name may only use letters, digits, - and _");
      const engine = pcbEngine();
      if (!engine) throw new ToolError("Forge's PCB engine is not installed: run npm run pcb-engine in the Forge folder");

      const lookedUp = !!input.lookup_parts;
      const rel = path.relative(ctx.projectDir, full).split(path.sep).join("/");
      const r = await runEngine(engine, ctx.projectDir, { source: rel, name, lookup_parts: lookedUp }, 5 * 60_000);
      const { result, error } = parseRunnerOutput(r);
      if (error) throw new ToolError(clip(`PCB engine: ${error}`));
      const res = result!;
      const b = res.board as { width: number; height: number; thickness: number; layers: number };
      const parts = res.components as string[];
      const errors = res.errors as string[];
      const warnings = summarizeWarnings(res.warnings as string[], lookedUp);
      const lines = [
        `Built ${rel}: board ${b.width} × ${b.height} mm, ${b.thickness} mm thick, ${b.layers} copper layers.`,
        `${parts.length} part(s): ${parts.slice(0, 60).join(", ")}${parts.length > 60 ? " …" : ""}`,
        `${res.connections} connection(s) and ${res.nets} named net(s), routed as ${res.traces} trace(s) with ${res.vias} via(s); ${res.holes} hole(s).`,
      ];
      const pours = (res.pours as string[] | undefined) ?? [];
      if (pours.length || res.keepouts) lines.push(`Copper pours: ${pours.length ? pours.join(", ") : "none"}; ${res.keepouts ?? 0} keepout area(s).`);
      if (errors.length) lines.push(`${errors.length} ERROR(S), fix them and rebuild:`, ...errors.slice(0, 30).map((e) => `  ${e}`));
      else lines.push("No errors: every connection is routed.");
      if (warnings.length) lines.push("Warnings:", ...warnings.slice(0, 20).map((w) => `  ${w}`));
      if (res.schematicNote) lines.push(String(res.schematicNote));
      const files = (res.files as string[]).filter((f) => f.startsWith("pcb/"));
      lines.push(`Files: ${files.join(", ")}, fab/${name}/ (Gerbers, drills, pick-and-place, BOM)`);
      return { text: clip(lines.join("\n")), files: [...files, `fab/${name}/`] };
    },
  },
];
