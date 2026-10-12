import { report, run } from "./exec.ts";
import { ToolError, type ForgeTool } from "./types.ts";

/** Commits as Forge when the user has no git identity configured, so commits never fail on a fresh machine. */
async function identity(cwd: string): Promise<string[]> {
  const email = await run("git", ["config", "user.email"], cwd);
  return email.code === 0 && email.stdout.trim() ? [] : ["-c", "user.name=Forge", "-c", "user.email=forge@localhost"];
}

export const vcsTools: ForgeTool[] = [
  {
    domain: "vcs",
    name: "git_status",
    description: "Show which project files changed since the last commit.",
    input_schema: { type: "object", properties: {} },
    requires: "git",
    async run(_input, ctx) {
      const r = await run("git", ["status", "--short"], ctx.projectDir);
      return { text: r.code === 0 ? r.stdout.trim() || "working tree clean" : report(r) };
    },
  },
  {
    domain: "vcs",
    name: "git_commit",
    description:
      "Record every current change in the project's history with a message. Commit after each meaningful step so the user can review and roll back.",
    input_schema: {
      type: "object",
      properties: { message: { type: "string", description: "What changed and why, one line" } },
      required: ["message"],
    },
    requires: "git",
    async run(input, ctx) {
      const cwd = ctx.projectDir;
      await run("git", ["add", "-A"], cwd);
      const staged = await run("git", ["diff", "--cached", "--quiet"], cwd);
      if (staged.code === 0) return { text: "nothing to commit" };
      const r = await run("git", [...(await identity(cwd)), "commit", "-m", input.message as string], cwd);
      if (r.code !== 0) throw new ToolError(report(r));
      const head = await run("git", ["log", "-1", "--format=%h %s"], cwd);
      return { text: `committed ${head.stdout.trim()}` };
    },
  },
  {
    domain: "vcs",
    name: "git_log",
    description: "Show the most recent commits in the project.",
    input_schema: {
      type: "object",
      properties: { limit: { type: "integer", description: "How many commits (default 20)" } },
    },
    requires: "git",
    async run(input, ctx) {
      const limit = Math.min(Math.max(Number(input.limit ?? 20), 1), 200);
      const r = await run("git", ["log", `-${limit}`, "--format=%h %ad %s", "--date=short"], ctx.projectDir);
      return { text: r.code === 0 ? r.stdout.trim() || "no commits yet" : report(r) };
    },
  },
];

/** Creates the repository for a new project with an initial commit. */
export async function initRepo(cwd: string): Promise<void> {
  await run("git", ["init", "-q"], cwd);
  await run("git", ["add", "-A"], cwd);
  await run("git", [...(await identity(cwd)), "commit", "-q", "-m", "Start project"], cwd);
}
