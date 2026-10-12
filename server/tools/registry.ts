import { cadTools } from "./cad.ts";
import { chipTools } from "./chip.ts";
import { codeTools, commandsAllowed } from "./code.ts";
import { findProgram } from "./exec.ts";
import { pcbTools } from "./pcb.ts";
import { script3dTools } from "./script3d.ts";
import type { ForgeTool } from "./types.ts";
import { vcsTools } from "./vcs.ts";
import { workspaceTools } from "./workspace.ts";

/** Every capability Forge knows about. Add a domain by appending its tool list here. */
export const allTools: ForgeTool[] = [...workspaceTools, ...vcsTools, ...codeTools, ...script3dTools, ...cadTools, ...pcbTools, ...chipTools];

export function isAvailable(tool: ForgeTool): boolean {
  if (tool.name === "run_command") return commandsAllowed();
  return !tool.requires || findProgram(tool.requires) !== undefined;
}

/** The tools the agent may call right now: those whose external programs are installed. */
export function availableTools(): ForgeTool[] {
  return allTools.filter(isAvailable);
}

export function describeTools() {
  return allTools.map((t) => ({
    name: t.name,
    domain: t.domain,
    description: t.description,
    available: isAvailable(t),
    requires: t.name === "run_command" ? "FORGE_ALLOW_COMMANDS=1" : [t.requires ?? []].flat().at(-1),
    install: t.install,
  }));
}
