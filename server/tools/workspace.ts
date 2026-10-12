import fs from "node:fs/promises";
import path from "node:path";
import { projectPath } from "./paths.ts";
import { ToolError, type ForgeTool } from "./types.ts";

const MAX_READ = 200_000;

/** Lists project files (relative, forward slashes), skipping .git and node_modules. */
export async function listFiles(projectDir: string, dir = ""): Promise<string[]> {
  const entries = await fs.readdir(path.join(projectDir, dir), { withFileTypes: true });
  const out: string[] = [];
  for (const e of entries.sort((a, b) => a.name.localeCompare(b.name))) {
    if (e.name === ".git" || e.name === "node_modules") continue;
    const rel = dir ? `${dir}/${e.name}` : e.name;
    if (e.isDirectory()) out.push(...(await listFiles(projectDir, rel)));
    else out.push(rel);
  }
  return out;
}

export const workspaceTools: ForgeTool[] = [
  {
    domain: "workspace",
    name: "list_files",
    description: "List every file in the project (paths relative to the project root).",
    input_schema: { type: "object", properties: {} },
    async run(_input, ctx) {
      const files = await listFiles(ctx.projectDir);
      return { text: files.length ? files.join("\n") : "(the project is empty)" };
    },
  },
  {
    domain: "workspace",
    name: "read_file",
    description: "Read a text file from the project.",
    input_schema: {
      type: "object",
      properties: { path: { type: "string", description: "Project-relative path" } },
      required: ["path"],
    },
    async run(input, ctx) {
      const full = projectPath(ctx.projectDir, input.path as string);
      const stat = await fs.stat(full).catch(() => null);
      if (!stat?.isFile()) throw new ToolError(`${input.path} does not exist`);
      if (stat.size > MAX_READ) throw new ToolError(`${input.path} is ${stat.size} bytes; only files up to ${MAX_READ} bytes can be read`);
      return { text: await fs.readFile(full, "utf8") };
    },
  },
  {
    domain: "workspace",
    name: "write_file",
    description:
      "Create or overwrite a text file in the project. Use it for source code, OpenSCAD, Verilog, notes, BOMs and any other text artifact. Parent folders are created.",
    input_schema: {
      type: "object",
      properties: {
        path: { type: "string", description: "Project-relative path" },
        contents: { type: "string", description: "The full file contents" },
      },
      required: ["path", "contents"],
    },
    async run(input, ctx) {
      const rel = input.path as string;
      const full = projectPath(ctx.projectDir, rel);
      await fs.mkdir(path.dirname(full), { recursive: true });
      await fs.writeFile(full, input.contents as string);
      return { text: `wrote ${rel} (${Buffer.byteLength(input.contents as string)} bytes)`, files: [rel] };
    },
  },
];
