import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { clip, run, type ExecResult } from "./exec.ts";
import { launchArgs, resolveApp, type AppSpec } from "./launch.ts";
import { projectPath } from "./paths.ts";
import { ToolError, type ForgeTool } from "./types.ts";

const RUNNERS = path.join(path.dirname(fileURLToPath(import.meta.url)), "runners");
const NAME = /^[A-Za-z0-9_-]{1,64}$/;

const defined = (list: (string | undefined)[]) => list.filter((p): p is string => !!p);

export const BLENDER: AppSpec = {
  label: "blender",
  native: defined([process.env.FORGE_BLENDER, "blender"]),
  flatpak: { id: "org.blender.Blender", commands: ["blender"] },
};

// FreeCAD 1.x names the console program freecadcmd; 0.x used FreeCADCmd.
export const FREECAD: AppSpec = {
  label: "freecadcmd",
  native: defined([process.env.FORGE_FREECAD, "freecadcmd", "FreeCADCmd", "freecad.cmd"]),
  flatpak: { id: "org.freecad.FreeCAD", commands: ["freecadcmd", "FreeCADCmd"] },
};

/** Runs a runner script in `spec`'s program. A Flatpak only gets the project (read-write) and the runners (read-only). */
function runApp(spec: AppSpec, args: string[], projectDir: string, timeoutMs: number, env: Record<string, string>) {
  const app = resolveApp(spec);
  if (!app) throw new ToolError(`${spec.label} is not installed`);
  const launch = launchArgs(app, args, { rw: [projectDir], ro: [RUNNERS], env });
  return run(launch.cmd, launch.args, projectDir, timeoutMs, launch.env);
}

/** Reads the runner's FORGE_RESULT / FORGE_ERROR line out of the program's log. */
export function parseRunnerOutput(r: ExecResult): { result?: Record<string, unknown>; error?: string } {
  for (const line of r.stdout.split("\n").reverse()) {
    if (line.startsWith("FORGE_RESULT ")) return { result: JSON.parse(line.slice(13)) };
    if (line.startsWith("FORGE_ERROR ")) return { error: JSON.parse(line.slice(12)) };
  }
  const log = `${r.stdout}\n${r.stderr}`.trim().split("\n").slice(-40).join("\n");
  return { error: `the program exited with code ${r.code} without a result. Last output:\n${log}` };
}

async function checkInputs(input: Record<string, unknown>, projectDir: string, allowed: string[]) {
  const name = input.name as string;
  if (!NAME.test(name)) throw new ToolError("name may only use letters, digits, - and _");
  const script = input.script as string;
  const full = projectPath(projectDir, script);
  if (!(await fs.stat(full).catch(() => null))?.isFile()) throw new ToolError(`${script} does not exist; write it with write_file first`);
  const formats = (input.formats as string[] | undefined) ?? allowed;
  const unknown = formats.filter((f) => !allowed.includes(f));
  if (unknown.length) throw new ToolError(`unsupported formats: ${unknown.join(", ")}`);
  return { name, script: full, formats };
}

const common = {
  script: { type: "string", description: "Project-relative .py file written beforehand with write_file" },
  name: { type: "string", description: "Base name for output files: letters, digits, - and _" },
};

export const script3dTools: ForgeTool[] = [
  {
    domain: "3d",
    name: "blender_build",
    description: `Run a Blender Python (bpy) script headless to build visual 3D: product renders, organic or sculpted shapes, scenes, materials, artwork, game assets. Not for parts that need exact engineering dimensions (use freecad_build).
Write the script with write_file first. It runs in an empty scene with the project folder as working directory; create objects, modifiers, materials, and optionally lights and a camera. Afterwards every object is exported to models/<name>.glb (shown in Forge's 3D viewer) plus any other requested formats. With render=true a 960×720 PNG is rendered to renders/<name>.png with Cycles on the CPU; if the script adds no camera or lights, Forge frames the scene and lights it. Blender units are metres by default.`,
    input_schema: {
      type: "object",
      properties: {
        ...common,
        formats: {
          type: "array",
          items: { type: "string", enum: ["glb", "stl", "obj", "blend"] },
          description: 'Export formats (default ["glb"]). stl for 3D printing, blend to keep an editable Blender file.',
        },
        render: { type: "boolean", description: "Also render a PNG (takes longer)" },
      },
      required: ["script", "name"],
    },
    app: BLENDER,
    install: { dnf: "blender python3-numpy", apt: "blender python3-numpy", brew: "--cask blender", flatpak: "org.blender.Blender", url: "https://www.blender.org/download/" },
    async run(input, ctx) {
      const { name, script, formats } = await checkInputs(input, ctx.projectDir, ["glb", "stl", "obj", "blend"]);
      const chosen = (input.formats as string[] | undefined) ? formats : ["glb"];
      const r = await runApp(
        BLENDER,
        ["--background", "--factory-startup", "--python-exit-code", "1", "--python", path.join(RUNNERS, "blender_runner.py")],
        ctx.projectDir,
        input.render ? 15 * 60_000 : 5 * 60_000,
        { PYTHONDONTWRITEBYTECODE: "1", FORGE_PROJECT: ctx.projectDir, FORGE_SCRIPT: script, FORGE_NAME: name, FORGE_FORMATS: chosen.join(","), FORGE_RENDER: input.render ? "1" : "0" },
      );
      const { result, error } = parseRunnerOutput(r);
      if (error) throw new ToolError(clip(`Blender: ${error}`));
      const files = result!.files as string[];
      const size = (result!.size as number[]).join(" × ");
      return {
        text: `Blender built ${(result!.objects as string[]).length} objects (${result!.vertices} vertices, ${result!.faces} faces), overall ${size} units.\nFiles: ${files.join(", ")}`,
        files,
      };
    },
  },
  {
    domain: "3d",
    name: "freecad_build",
    description: `Run a FreeCAD Python script headless to build an engineering part with exact dimensions: brackets, enclosures, mounts, shafts, anything with holes, fits, fillets or chamfers that will be machined or printed. Units are millimetres.
Write the script with write_file first. It receives App (FreeCAD), Part and a new document doc. Build solids with Part (makeBox, makeCylinder, cut, fuse, makeFillet, extrude ...) and either assign result = <shape> or add objects to doc. Forge exports models/<name>.step (for CNC shops and other CAD), models/<name>.stl (3D viewer and printing) and models/<name>.FCStd (editable in FreeCAD), and reports whether the solid is valid, its volume and bounding box: check them against the requirements.`,
    input_schema: {
      type: "object",
      properties: {
        ...common,
        formats: {
          type: "array",
          items: { type: "string", enum: ["step", "stl", "fcstd"] },
          description: "Export formats (default all three)",
        },
      },
      required: ["script", "name"],
    },
    app: FREECAD,
    // Fedora's repositories have no FreeCAD package; Flathub carries the official build.
    install: { flatpak: "org.freecad.FreeCAD", apt: "freecad", brew: "--cask freecad", url: "https://www.freecad.org/downloads.php" },
    async run(input, ctx) {
      const { name, script, formats } = await checkInputs(input, ctx.projectDir, ["step", "stl", "fcstd"]);
      const r = await runApp(FREECAD, [path.join(RUNNERS, "freecad_runner.py")], ctx.projectDir, 5 * 60_000, {
        PYTHONDONTWRITEBYTECODE: "1",
        FORGE_PROJECT: ctx.projectDir,
        FORGE_SCRIPT: script,
        FORGE_NAME: name,
        FORGE_FORMATS: formats.join(","),
      });
      const { result, error } = parseRunnerOutput(r);
      if (error) throw new ToolError(clip(`FreeCAD: ${error}`));
      const res = result!;
      const files = res.files as string[];
      const warning = res.valid ? "" : "\nWARNING: the shape is not a valid solid; fix the geometry before manufacturing.";
      return {
        text: `FreeCAD built ${res.solids} solid(s): ${(res.size_mm as number[]).join(" × ")} mm, volume ${res.volume_mm3} mm³, valid: ${res.valid}.${warning}\nFiles: ${files.join(", ")}`,
        files,
      };
    },
  },
];
