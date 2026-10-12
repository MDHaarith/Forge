import type Anthropic from "@anthropic-ai/sdk";

/** The workshop areas a tool belongs to. The UI groups capabilities by these. */
export type Domain = "workspace" | "vcs" | "code" | "3d" | "pcb" | "chip";

export interface ToolContext {
  /** Absolute path of the open project. Tools must not touch anything outside it. */
  projectDir: string;
}

export interface ToolResult {
  text: string;
  /** Project-relative files the tool created or changed, so the UI can refresh and preview them. */
  files?: string[];
}

/**
 * A capability Forge can hand to the agent. Every domain plugin (CAD, PCB, chip, ...)
 * is a list of these. Tools that wrap an external program name it in `requires`;
 * they are hidden from the agent until that program is on PATH.
 */
export interface ForgeTool {
  domain: Domain;
  name: string;
  description: string;
  input_schema: Anthropic.Tool.InputSchema;
  /** Program(s) the tool runs; the first one found is used. Unset for built-in tools. */
  requires?: string | string[];
  install?: string;
  run(input: Record<string, unknown>, ctx: ToolContext): Promise<ToolResult>;
}

/** Thrown by tools for expected failures; the message is shown to the agent as-is. */
export class ToolError extends Error {}
