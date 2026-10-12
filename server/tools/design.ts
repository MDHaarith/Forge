import fs from "node:fs/promises";
import { apply, boardBounds, fitCheck, parseStl, rotationMatrix, type BoardBoxes, type FitIssue, type Placement, type Vec } from "./fit.ts";
import { projectPath } from "./paths.ts";
import { ToolError, type ForgeTool } from "./types.ts";

const NAME = /^[A-Za-z0-9_-]{1,64}$/;
const COLOR = /^#[0-9a-fA-F]{6}$/;

export interface DesignPart extends Placement {
  file: string;
  color?: string;
  opacity?: number;
}

/** What design/<name>.json holds: the parts the Design view shows together, and the last fit check. */
export interface Design {
  parts: DesignPart[];
  clearance: number;
  fit: FitIssue[];
}

function vec(value: unknown, what: string): Vec {
  if (value === undefined) return [0, 0, 0];
  if (!Array.isArray(value) || value.length !== 3 || !value.every((n) => typeof n === "number" && Number.isFinite(n))) throw new ToolError(`${what} must be three numbers`);
  return value as Vec;
}

const exists = async (full: string) => !!(await fs.stat(full).catch(() => null))?.isFile();

/** The board data next to a board's 3D model: pcb/main.glb -> pcb/main.board.json. */
export const boardDataFor = (file: string) => (/\.glb$/i.test(file) ? file.replace(/\.glb$/i, ".board.json") : undefined);

async function checkPart(projectDir: string, raw: Record<string, unknown>): Promise<DesignPart> {
  let file = String(raw.file ?? "");
  // A STEP or FreeCAD file is shown and checked through the mesh freecad_build writes next to it.
  if (/\.(step|stp|fcstd)$/i.test(file)) file = file.replace(/\.[^.]+$/, ".stl");
  if (!/\.(stl|glb)$/i.test(file)) throw new ToolError(`${raw.file}: parts must be .stl or .glb models (or a .step with its .stl beside it)`);
  if (!(await exists(projectPath(projectDir, file)))) throw new ToolError(`${file} does not exist; build it first (pcb_build for a board, freecad_build for an enclosure)`);
  const part: DesignPart = { file, position: vec(raw.position, `${file} position`), rotation: vec(raw.rotation, `${file} rotation`) };
  if (raw.color !== undefined) {
    if (typeof raw.color !== "string" || !COLOR.test(raw.color)) throw new ToolError(`${file} color must look like #8899aa`);
    part.color = raw.color;
  }
  if (raw.opacity !== undefined) {
    const o = Number(raw.opacity);
    if (!(o > 0 && o <= 1)) throw new ToolError(`${file} opacity must be above 0 and at most 1`);
    part.opacity = o;
  }
  return part;
}

const fmt = (v: number[]) => v.map((n) => Math.round(n * 100) / 100).join(", ");

export const designTools: ForgeTool[] = [
  {
    domain: "pcb",
    name: "design_assemble",
    description: `Put circuit boards and mechanical parts together in one 3D assembly, like an electronics + mechanical design in Fusion 360, and check that the boards fit. Writes design/<name>.json, which the Design view shows: the board's PCB layout beside the 3D assembly.
Parts are board models from pcb_build (pcb/<name>.glb) and enclosure parts from freecad_build (.stl, or the .step with its .stl beside it), each placed with position [x, y, z] in mm and rotation [rx, ry, rz] in degrees. A board's origin is its centre, at half its thickness. FreeCAD parts keep their own coordinates.
The fit check tests the board and every component's 3D box against every mechanical part: a collision means a part cuts into the enclosure (move the board, the part or the enclosure feature); tight means closer than the clearance. It also reports the space each board takes up, to size or adjust an enclosure around it. Re-run it after changing either side.`,
    input_schema: {
      type: "object",
      properties: {
        name: { type: "string", description: "Assembly name: letters, digits, - and _" },
        parts: {
          type: "array",
          minItems: 1,
          items: {
            type: "object",
            properties: {
              file: { type: "string", description: "Project-relative .glb (board) or .stl/.step (mechanical)" },
              position: { type: "array", items: { type: "number" }, description: "[x, y, z] in mm (default 0, 0, 0)" },
              rotation: { type: "array", items: { type: "number" }, description: "[rx, ry, rz] in degrees (default 0, 0, 0)" },
              color: { type: "string", description: "Display colour such as #b0b8c8 (mechanical parts)" },
              opacity: { type: "number", description: "Display opacity, e.g. 0.35 to see a board through its case (default 1)" },
            },
            required: ["file"],
          },
        },
        clearance: { type: "number", description: "Gap in mm closer than which parts are reported as tight (default 0.5)" },
      },
      required: ["name", "parts"],
    },
    async run(input, ctx) {
      const name = String(input.name ?? "");
      if (!NAME.test(name)) throw new ToolError("name may only use letters, digits, - and _");
      const raw = input.parts as Record<string, unknown>[];
      if (!Array.isArray(raw) || !raw.length || raw.length > 50) throw new ToolError("give 1 to 50 parts");
      const clearance = input.clearance === undefined ? 0.5 : Number(input.clearance);
      if (!(clearance >= 0 && clearance <= 20)) throw new ToolError("clearance must be between 0 and 20 mm");
      const parts: DesignPart[] = [];
      for (const p of raw) parts.push(await checkPart(ctx.projectDir, p));

      const boards: { name: string; placement: Placement; board: BoardBoxes }[] = [];
      const solids: { name: string; placement: Placement; triangles: Float32Array }[] = [];
      const shownOnly: string[] = [];
      for (const p of parts) {
        const data = boardDataFor(p.file);
        if (data && (await exists(projectPath(ctx.projectDir, data)))) {
          boards.push({ name: p.file, placement: p, board: JSON.parse(await fs.readFile(projectPath(ctx.projectDir, data), "utf8")) });
        } else if (/\.stl$/i.test(p.file)) {
          try {
            solids.push({ name: p.file, placement: p, triangles: parseStl(await fs.readFile(projectPath(ctx.projectDir, p.file))) });
          } catch (e) {
            throw new ToolError(`${p.file}: ${(e as Error).message}`);
          }
        } else {
          shownOnly.push(p.file);
        }
      }

      const fit = fitCheck(boards, solids, clearance);
      const design: Design = { parts, clearance, fit };
      const rel = `design/${name}.json`;
      const full = projectPath(ctx.projectDir, rel);
      await fs.mkdir(projectPath(ctx.projectDir, "design"), { recursive: true });
      await fs.writeFile(full, JSON.stringify(design, null, 2) + "\n");

      const lines = [`Assembly ${rel}: ${parts.length} part(s), ${boards.length} board(s).`];
      for (const b of boards) {
        const { min, max } = boardBounds(b.board, b.placement);
        const tallest = [...b.board.components].sort((x, y) => y.max[2] - x.max[2])[0];
        lines.push(`  ${b.name} with its parts spans x ${fmt([min[0], max[0]])}, y ${fmt([min[1], max[1]])}, z ${fmt([min[2], max[2]])} mm${tallest ? `; tallest part ${tallest.name} reaches ${fmt([tallest.max[2] - b.board.size[2] / 2])} mm above the board` : ""}`);
      }
      for (const s of solids) {
        const m = rotationMatrix(s.placement.rotation);
        const min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
        for (let i = 0; i < s.triangles.length; i += 3) {
          const p = apply(m, s.placement.position, [s.triangles[i], s.triangles[i + 1], s.triangles[i + 2]]);
          for (let k = 0; k < 3; k++) {
            min[k] = Math.min(min[k], p[k]);
            max[k] = Math.max(max[k], p[k]);
          }
        }
        lines.push(`  ${s.name} spans x ${fmt([min[0], max[0]])}, y ${fmt([min[1], max[1]])}, z ${fmt([min[2], max[2]])} mm`);
      }
      if (shownOnly.length) lines.push(`  Shown but not checked (not an STL or a board): ${shownOnly.join(", ")}`);
      if (!boards.length || !solids.length) lines.push("Fit check: needs at least one board and one mechanical part.");
      else {
        const collisions = fit.filter((f) => f.status === "collision");
        const tight = fit.filter((f) => f.status === "tight");
        if (!fit.length) lines.push(`Fit check: everything clears by at least ${clearance} mm.`);
        if (collisions.length) lines.push(`Fit check: ${collisions.length} COLLISION(S):`, ...collisions.map((f) => `  ${f.item === "board" ? "the board" : f.item} on ${f.board} cuts into ${f.part}`));
        if (tight.length) lines.push(`Within ${clearance} mm (tight, check it is intended, e.g. the board resting on standoffs):`, ...tight.map((f) => `  ${f.item === "board" ? "the board" : f.item} on ${f.board} and ${f.part}`));
      }
      return { text: lines.join("\n"), files: [rel] };
    },
  },
];
