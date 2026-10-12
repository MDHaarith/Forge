import { execFile, spawnSync } from "node:child_process";

export interface ExecResult {
  code: number;
  stdout: string;
  stderr: string;
}

/** Runs a program without a shell. Never rejects: a failing exit code is part of the result. */
export function run(cmd: string, args: string[], cwd: string, timeoutMs = 120_000): Promise<ExecResult> {
  return new Promise((resolve) => {
    execFile(cmd, args, { cwd, timeout: timeoutMs, maxBuffer: 8 * 1024 * 1024 }, (error, stdout, stderr) => {
      const code = error ? (typeof error.code === "number" ? error.code : 1) : 0;
      resolve({ code, stdout: String(stdout), stderr: String(stderr || (error && !stderr ? error.message : "")) });
    });
  });
}

const found = new Map<string, boolean>();

/** True when `program` is on PATH. Cached for the life of the process. */
export function onPath(program: string): boolean {
  if (!found.has(program)) {
    const which = process.platform === "win32" ? "where" : "which";
    found.set(program, spawnSync(which, [program], { stdio: "ignore" }).status === 0);
  }
  return found.get(program)!;
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
