import { spawnSync } from "node:child_process";
import { findProgram, onPath } from "./exec.ts";

/**
 * A desktop program Forge drives headless, installed either normally (dnf, a tarball, an
 * AppImage) or as a Flatpak. Fedora Workstation ships Flatpak, and some programs (FreeCAD)
 * are only packaged there, so both have to work.
 */
export interface AppSpec {
  /** Shown in the UI when the program is missing. */
  label: string;
  /** Executable names or paths, tried in order. */
  native: string[];
  flatpak?: { id: string; commands: string[] };
}

export type Resolved = { kind: "native"; program: string } | { kind: "flatpak"; id: string; command: string };

export interface Launch {
  cmd: string;
  args: string[];
  env: Record<string, string>;
}

const flatpakCache = new Map<string, string | null>();

/** Finds the command inside an installed Flatpak app, e.g. freecadcmd in org.freecad.FreeCAD. Cached. */
function flatpakCommand(id: string, commands: string[]): string | undefined {
  const key = `${id}:${commands.join(",")}`;
  if (!flatpakCache.has(key)) {
    let found: string | null = null;
    if (process.env.FORGE_NO_FLATPAK !== "1" && onPath("flatpak") && spawnSync("flatpak", ["info", id], { stdio: "ignore" }).status === 0) {
      const probe = commands.map((c) => `command -v ${c} >/dev/null 2>&1 && { echo ${c}; exit 0; }`).join("; ") + "; exit 1";
      const r = spawnSync("flatpak", ["run", "--command=sh", id, "-c", probe], { encoding: "utf8", timeout: 60_000 });
      if (r.status === 0) found = r.stdout.trim().split("\n").pop() || null;
    }
    flatpakCache.set(key, found);
  }
  return flatpakCache.get(key) ?? undefined;
}

/** A normal install wins over a Flatpak: it starts faster and sees the whole filesystem. */
export function resolveApp(spec: AppSpec): Resolved | undefined {
  const program = findProgram(spec.native);
  if (program) return { kind: "native", program };
  if (spec.flatpak) {
    const command = flatpakCommand(spec.flatpak.id, spec.flatpak.commands);
    if (command) return { kind: "flatpak", id: spec.flatpak.id, command };
  }
  return undefined;
}

/**
 * Builds the command line. A Flatpak is sandboxed, so it gets explicit access to the folders it
 * must write (`rw`) and read (`ro`), and its environment is passed with --env.
 */
export function launchArgs(
  app: Resolved,
  args: string[],
  opts: { rw: string[]; ro: string[]; env: Record<string, string> },
): Launch {
  if (app.kind === "native") return { cmd: app.program, args, env: opts.env };
  return {
    cmd: "flatpak",
    args: [
      "run",
      `--command=${app.command}`,
      ...opts.rw.map((d) => `--filesystem=${d}`),
      ...opts.ro.map((d) => `--filesystem=${d}:ro`),
      ...Object.entries(opts.env).map(([k, v]) => `--env=${k}=${v}`),
      app.id,
      ...args,
    ],
    env: opts.env,
  };
}
