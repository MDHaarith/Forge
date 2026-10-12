import { execFile, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";

export interface ExecResult {
  code: number;
  stdout: string;
  stderr: string;
}

/** Runs a program without a shell. Never rejects: a failing exit code is part of the result. */
export function run(
  cmd: string,
  args: string[],
  cwd: string,
  timeoutMs = 120_000,
  env?: Record<string, string>,
): Promise<ExecResult> {
  return new Promise((resolve) => {
    const options = { cwd, timeout: timeoutMs, maxBuffer: 32 * 1024 * 1024, env: env ? { ...process.env, ...env } : undefined };
    execFile(cmd, args, options, (error, stdout, stderr) => {
      const code = error ? (typeof error.code === "number" ? error.code : 1) : 0;
      resolve({ code, stdout: String(stdout), stderr: String(stderr || (error && !stderr ? error.message : "")) });
    });
  });
}

const found = new Map<string, boolean>();

/** True when `program` is on PATH, or is a path to an existing file. Cached for the life of the process. */
export function onPath(program: string): boolean {
  if (!found.has(program)) {
    if (program.includes("/") || program.includes("\\")) found.set(program, existsSync(program));
    else {
      const which = process.platform === "win32" ? "where" : "which";
      found.set(program, spawnSync(which, [program], { stdio: "ignore" }).status === 0);
    }
  }
  return found.get(program)!;
}

/** The first of `candidates` that can be run, or undefined. Lets one tool accept several program names. */
export function findProgram(candidates: string | string[] | undefined): string | undefined {
  return [candidates ?? []].flat().find(onPath);
}

/** Trims long tool output so one noisy command can't flood the agent's context. */
export function clip(text: string, max = 12_000): string {
  return text.length <= max ? text : `${text.slice(0, max / 2)}\n... (${text.length - max} characters cut) ...\n${text.slice(-max / 2)}`;
}

export function report(r: ExecResult): string {
  const parts = [`exit code ${r.code}`];
  if (r.stdout.trim()) parts.push(`stdout:\n${r.stdout.trim()}`);
  if (r.stderr.trim()) parts.push(`stderr:\n${r.stderr.trim()}`);
  return clip(parts.join("\n"));
}
