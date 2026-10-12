import { report, run } from "./exec.ts";
import { ToolError, type ForgeTool } from "./types.ts";

/**
 * Running arbitrary commands is off unless the user opts in with FORGE_ALLOW_COMMANDS=1,
 * because the agent's output then executes on their machine.
 */
export function commandsAllowed(): boolean {
  return process.env.FORGE_ALLOW_COMMANDS === "1";
}

export const codeTools: ForgeTool[] = [
  {
    domain: "code",
    name: "run_command",
    description:
      "Run a program inside the project folder (no shell: pass the program and its arguments separately), e.g. to build or test code. Times out after 2 minutes.",
    input_schema: {
      type: "object",
      properties: {
        program: { type: "string", description: "Executable name, e.g. npm, python3, make" },
        args: { type: "array", items: { type: "string" }, description: "Arguments" },
      },
      required: ["program"],
    },
    async run(input, ctx) {
      if (!commandsAllowed()) throw new ToolError("running commands is disabled; start Forge with FORGE_ALLOW_COMMANDS=1 to allow it");
      const r = await run(input.program as string, (input.args as string[] | undefined) ?? [], ctx.projectDir);
      return { text: report(r) };
    },
  },
];
