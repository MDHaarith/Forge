import fs from "node:fs/promises";
import path from "node:path";
import { clip, findProgram, run } from "./exec.ts";
import { projectPath } from "./paths.ts";
import { FREECAD, parseRunnerOutput, RUNNERS, runApp } from "./script3d.ts";
import { ToolError, type ForgeTool } from "./types.ts";

const NAME = /^[A-Za-z0-9_-]{1,64}$/;
const MODEL_TYPES = [".step", ".stp", ".brep", ".fcstd", ".iges", ".igs"];

// Keep in step with MATERIALS in runners/forge_cad.py.
export const MATERIALS = [
  "PLA",
  "PETG",
  "ABS",
  "ASA",
  "Nylon PA12",
  "Polycarbonate",
  "Acrylic",
  "POM (Delrin)",
  "Aluminium 6061",
  "Aluminium 7075",
  "Steel",
  "Stainless 304",
  "Brass",
  "Copper",
  "Titanium Ti-6Al-4V",
  "Plywood",
];

async function checkModel(projectDir: string, rel: unknown) {
  if (typeof rel !== "string") throw new ToolError("model paths must be strings");
  const full = projectPath(projectDir, rel);
  if (!MODEL_TYPES.includes(path.extname(full).toLowerCase())) throw new ToolError(`${rel}: use a STEP, BREP, IGES or FCStd file (an STL mesh has no exact geometry)`);
  if (!(await fs.stat(full).catch(() => null))?.isFile()) throw new ToolError(`${rel} does not exist; build it with freecad_build first`);
  return rel;
}

function checkMaterial(material: unknown) {
  if (material === undefined) return undefined;
  const m = MATERIALS.find((x) => x.toLowerCase() === String(material).toLowerCase());
  if (!m) throw new ToolError(`unknown material ${material}; use one of: ${MATERIALS.join(", ")}`);
  return m;
}

async function runFreecad(runner: string, projectDir: string, args: Record<string, unknown>) {
  const r = await runApp(FREECAD, [path.join(RUNNERS, runner)], projectDir, 5 * 60_000, {
    PYTHONDONTWRITEBYTECODE: "1",
    FORGE_PROJECT: projectDir,
    FORGE_ARGS: JSON.stringify(args),
  });
  const { result, error } = parseRunnerOutput(r);
  if (error) throw new ToolError(clip(`FreeCAD: ${error}`));
  return result!;
}

interface Hole {
  axis: string;
  diameter: number;
  through: boolean;
  depth: number;
  center: number[];
  counterbore?: number[];
  countersink?: number;
  fits?: string | null;
}

function holeLine(h: Hole) {
  let s = `Ø${h.diameter} ${h.through ? "through" : `${h.depth} deep`} along ${h.axis} at (${h.center.join(", ")})`;
  if (h.counterbore) s += `, counterbore Ø${h.counterbore[0]} × ${h.counterbore[1]}`;
  if (h.countersink) s += `, countersink Ø${h.countersink}`;
  if (h.fits) s += ` [${h.fits}]`;
  return s;
}

export const freecadTools: ForgeTool[] = [
  {
    domain: "3d",
    name: "freecad_inspect",
    description: `Measure FreeCAD parts exactly: validity, volume, mass in a chosen material, bounding box, centre of mass, every hole (diameter, through or depth, counterbore/countersink, which screw it fits), 3D-print overhang area for the current orientation, and interference or clearance between parts (each solid in each file is a part, so an assembly can be checked). Use it to verify a design against the requirements before calling it done, and to check that parts fit together.`,
    input_schema: {
      type: "object",
      properties: {
        models: { type: "array", items: { type: "string" }, minItems: 1, description: "Project-relative .step/.FCStd files" },
        material: { type: "string", enum: MATERIALS, description: "Material for the mass" },
        overhang: { type: "number", description: "Steepest printable overhang from vertical in degrees (default 45)" },
      },
      required: ["models"],
    },
    app: FREECAD,
    install: { flatpak: "org.freecad.FreeCAD", apt: "freecad", brew: "--cask freecad", url: "https://www.freecad.org/downloads.php" },
    async run(input, ctx) {
      const models = input.models as unknown[];
      if (!Array.isArray(models) || !models.length || models.length > 20) throw new ToolError("give 1 to 20 models");
      for (const m of models) await checkModel(ctx.projectDir, m);
      const material = checkMaterial(input.material);
      const res = await runFreecad("freecad_inspect.py", ctx.projectDir, { models, material, overhang: input.overhang ?? 45 });
      const lines: string[] = [];
      for (const p of res.parts as Record<string, unknown>[]) {
        lines.push(`${p.file}: ${p.solids} solid(s), valid: ${p.valid}${p.valid ? "" : " — fix the geometry before manufacturing"}`);
        lines.push(`  size ${(p.size_mm as number[]).join(" × ")} mm from (${(p.min_mm as number[]).join(", ")}), volume ${p.volume_mm3} mm³, area ${p.area_mm2} mm²`);
        if (p.mass_g !== undefined) lines.push(`  mass ${p.mass_g} g in ${res.material} (${res.density_g_cm3} g/cm³)`);
        if (p.center_of_mass_mm) lines.push(`  centre of mass (${(p.center_of_mass_mm as number[]).join(", ")})`);
        const holes = p.holes as Hole[];
        lines.push(holes.length ? `  ${holes.length} hole(s):` : "  no holes");
        for (const h of holes.slice(0, 40)) lines.push(`    ${holeLine(h)}`);
        if (holes.length > 40) lines.push(`    … and ${holes.length - 40} more`);
        if (p.overhang_mm2 !== undefined) {
          lines.push(`  printing as oriented: ${p.overhang_mm2} mm² overhangs steeper than ${res.overhang_angle}° (${p.overhang_percent}% of the surface), ${p.bed_contact_mm2} mm² on the bed`);
        }
      }
      const pairs = res.pairs as Record<string, unknown>[];
      if (pairs.length) {
        lines.push("Between parts (pairs within 5 mm):");
        for (const pr of pairs) {
          lines.push(
            pr.interference_mm3 !== undefined
              ? `  ${pr.a} and ${pr.b}: INTERFERE by ${pr.interference_mm3} mm³`
              : `  ${pr.a} and ${pr.b}: clearance ${pr.clearance_mm} mm${(pr.clearance_mm as number) < 0.2 ? " (touching or very tight)" : ""}`,
          );
        }
      }
      return { text: lines.join("\n") };
    },
  },
  {
    domain: "3d",
    name: "freecad_drawing",
    description: `Make a technical drawing of a FreeCAD part for a workshop or manufacturer: front, top and side views with hidden lines, an isometric view, overall dimensions, every hole tagged on the views and listed in a hole table (size, through/depth, counterbore, position from the part's corner, screw it fits), and a title block with title, material, mass, scale, sheet and projection symbol (general tolerance ISO 2768-m). Writes drawings/<name>.svg, drawings/<name>.pdf when rsvg-convert is installed, and with dxf=true drawings/<name>-top.dxf (the top outline at 1:1 with true arcs, for laser cutting or CNC).`,
    input_schema: {
      type: "object",
      properties: {
        model: { type: "string", description: "Project-relative .step or .FCStd file" },
        name: { type: "string", description: "Base name for the drawing files: letters, digits, - and _" },
        title: { type: "string", description: "Title in the title block (default: the name)" },
        material: { type: "string", enum: MATERIALS },
        projection: { type: "string", enum: ["first", "third"], description: "first angle (ISO, Europe, India; default) or third angle (USA)" },
        sheet: { type: "string", enum: ["auto", "A4", "A3", "A2"], description: "Sheet size (default: smallest that fits at 1:1 or larger)" },
        dxf: { type: "boolean", description: "Also export the top view as DXF" },
      },
      required: ["model", "name"],
    },
    app: FREECAD,
    install: { flatpak: "org.freecad.FreeCAD", apt: "freecad", brew: "--cask freecad", url: "https://www.freecad.org/downloads.php" },
    async run(input, ctx) {
      const name = String(input.name);
      if (!NAME.test(name)) throw new ToolError("name may only use letters, digits, - and _");
      const model = await checkModel(ctx.projectDir, input.model);
      const title = input.title === undefined ? undefined : String(input.title).slice(0, 60);
      const res = await runFreecad("freecad_drawing.py", ctx.projectDir, {
        model,
        name,
        title,
        material: checkMaterial(input.material),
        projection: input.projection ?? "first",
        sheet: input.sheet ?? "auto",
        dxf: !!input.dxf,
      });
      const files = res.files as string[];
      const svg = files[0];
      let pdfNote = "";
      const rsvg = findProgram(["rsvg-convert"]);
      if (rsvg) {
        const pdf = svg.replace(/\.svg$/, ".pdf");
        const r = await run(rsvg, ["-f", "pdf", "-o", projectPath(ctx.projectDir, pdf), projectPath(ctx.projectDir, svg)], ctx.projectDir, 60_000);
        if (r.code === 0) files.splice(1, 0, pdf);
        else pdfNote = `\nPDF export failed: ${clip(r.stderr, 500)}`;
      } else {
        pdfNote = "\nNo PDF: install rsvg-convert (Fedora: sudo dnf install librsvg2-tools) to also get a PDF.";
      }
      const holes = res.holes as { tag: string; hole: string; axis: string; at: string; for: string }[];
      const table = holes.map((h) => `  ${h.tag}: ${h.hole}, axis ${h.axis}, ${h.at}${h.for ? ` (${h.for})` : ""}`).join("\n");
      return {
        text: `Drawing on ${res.sheet} at ${res.scale}, ${res.projection} projection, part ${(res.size_mm as number[]).join(" × ")} mm${res.mass_g ? `, ${res.mass_g} g` : ""}.${holes.length ? `\nHole table:\n${table}` : ""}${pdfNote}\nFiles: ${files.join(", ")}`,
        files,
      };
    },
  },
];
