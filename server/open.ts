import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { installText } from "./platform.ts";
import type { InstallHint } from "./platform.ts";
import { launchArgs, resolveApp, type AppSpec } from "./tools/launch.ts";
import { projectPath } from "./tools/paths.ts";
import { BLENDER } from "./tools/script3d.ts";

const defined = (list: (string | undefined)[]) => list.filter((p): p is string => !!p);

interface Opener {
  name: string;
  types: string[];
  app: AppSpec;
  install: InstallHint;
}

/** Desktop programs that project files open in, so a part built by Forge can be refined by hand. */
export const OPENERS: Opener[] = [
  {
    name: "FreeCAD",
    types: [".fcstd", ".step", ".stp", ".iges", ".igs", ".brep"],
    app: {
      label: "freecad",
      native: defined([process.env.FORGE_FREECAD_GUI, "freecad", "FreeCAD"]),
      flatpak: { id: "org.freecad.FreeCAD", commands: ["freecad", "FreeCAD"] },
    },
    install: { flatpak: "org.freecad.FreeCAD", apt: "freecad", brew: "--cask freecad", url: "https://www.freecad.org/downloads.php" },
  },
  {
    name: "Blender",
    types: [".blend"],
    app: BLENDER,
    install: { dnf: "blender", apt: "blender", flatpak: "org.blender.Blender", brew: "--cask blender", url: "https://www.blender.org/download/" },
  },
  {
    name: "KiCad",
    types: [".kicad_pcb"],
    app: { label: "pcbnew", native: ["pcbnew"], flatpak: { id: "org.kicad.KiCad", commands: ["pcbnew"] } },
    install: { dnf: "kicad", apt: "kicad", flatpak: "org.kicad.KiCad", brew: "--cask kicad", url: "https://www.kicad.org/download/" },
  },
  {
    name: "KiCad",
    types: [".kicad_sch"],
    app: { label: "eeschema", native: ["eeschema"], flatpak: { id: "org.kicad.KiCad", commands: ["eeschema"] } },
    install: { dnf: "kicad", apt: "kicad", flatpak: "org.kicad.KiCad", brew: "--cask kicad", url: "https://www.kicad.org/download/" },
  },
];

export class OpenError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

/** Which installed program opens each file type, for the UI's "Open in …" button. */
export function openerTypes(): Record<string, string> {
  const types: Record<string, string> = {};
  for (const o of OPENERS) if (resolveApp(o.app)) for (const t of o.types) types[t] = o.name;
  return types;
}

/** Starts the program for a project file in the background and returns the program's name. */
export async function openInApp(projectDir: string, rel: string): Promise<string> {
  let full: string;
  try {
    full = projectPath(projectDir, rel);
  } catch (e) {
    throw new OpenError(400, (e as Error).message);
  }
  const opener = OPENERS.find((o) => o.types.includes(path.extname(full).toLowerCase()));
  if (!opener) throw new OpenError(400, `Forge does not know a program for ${path.basename(full)}`);
  if (!(await fs.stat(full).catch(() => null))?.isFile()) throw new OpenError(404, "no such file");
  const app = resolveApp(opener.app);
  if (!app) {
    const hint = installText(opener.install);
    throw new OpenError(501, `${opener.name} is not installed${hint ? `: ${hint}` : ""}`);
  }
  // A Flatpak gets the project folder only, read-write so the user can save their changes.
  const launch = launchArgs(app, [full], { rw: [projectDir], ro: [], env: {} });
  await new Promise<void>((resolve, reject) => {
    const child = spawn(launch.cmd, launch.args, { cwd: projectDir, detached: true, stdio: "ignore", env: { ...process.env, ...launch.env } });
    child.once("error", (e) => reject(new OpenError(500, `could not start ${opener.name}: ${e.message}`)));
    child.once("spawn", () => {
      child.unref();
      resolve();
    });
  });
  return opener.name;
}
