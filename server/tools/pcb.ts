import { report, run } from "./exec.ts";
import { projectPath } from "./paths.ts";
import { ToolError, type ForgeTool } from "./types.ts";

const board = {
  type: "object" as const,
  properties: { board: { type: "string", description: "Project-relative .kicad_pcb file" } },
  required: ["board"],
};

const KICAD = { dnf: "kicad", apt: "kicad", brew: "--cask kicad", url: "https://www.kicad.org/download/" };

export const pcbTools: ForgeTool[] = [
  {
    domain: "pcb",
    name: "pcb_drc",
    description: "Run KiCad's design rule check on a board and write the report to fab/drc.rpt.",
    input_schema: board,
    requires: "kicad-cli",
    install: KICAD,
    async run(input, ctx) {
      projectPath(ctx.projectDir, input.board as string);
      const r = await run("kicad-cli", ["pcb", "drc", "--output", "fab/drc.rpt", input.board as string], ctx.projectDir);
      return { text: report(r), files: ["fab/drc.rpt"] };
    },
  },
  {
    domain: "pcb",
    name: "pcb_export_fab",
    description:
      "Export manufacturing files for a board: Gerbers and drill files into fab/, ready to zip and upload to a PCB fab such as JLCPCB or PCBWay.",
    input_schema: board,
    requires: "kicad-cli",
    install: KICAD,
    async run(input, ctx) {
      projectPath(ctx.projectDir, input.board as string);
      const gerbers = await run("kicad-cli", ["pcb", "export", "gerbers", "--output", "fab/", input.board as string], ctx.projectDir);
      if (gerbers.code !== 0) throw new ToolError(report(gerbers));
      const drill = await run("kicad-cli", ["pcb", "export", "drill", "--output", "fab/", input.board as string], ctx.projectDir);
      return { text: `gerbers:\n${report(gerbers)}\n\ndrill:\n${report(drill)}`, files: ["fab/"] };
    },
  },
];
