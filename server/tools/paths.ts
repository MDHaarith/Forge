import path from "node:path";
import { ToolError } from "./types.ts";

/** Resolves a project-relative path, refusing anything that escapes the project or enters .git. */
export function projectPath(projectDir: string, relative: string): string {
  const root = path.resolve(projectDir);
  const full = path.resolve(root, relative);
  const rel = path.relative(root, full);
  if (rel.startsWith("..") || path.isAbsolute(rel)) {
    throw new ToolError(`path "${relative}" is outside the project`);
  }
  if (rel === ".git" || rel.startsWith(`.git${path.sep}`)) {
    throw new ToolError("the .git folder is managed by the git tools");
  }
  return full;
}
