import fs from "node:fs/promises";
import { report, run } from "./exec.ts";
import { projectPath } from "./paths.ts";
import { ToolError, type ForgeTool } from "./types.ts";

const MODULE = /^[A-Za-z_][A-Za-z0-9_$]*$/;

export const chipTools: ForgeTool[] = [
  {
    domain: "chip",
    name: "chip_simulate",
    description:
      "Compile Verilog sources with Icarus Verilog and run the testbench. Put $display / $finish in the testbench; its output is returned.",
    input_schema: {
      type: "object",
      properties: {
        sources: { type: "array", items: { type: "string" }, minItems: 1, description: "Project-relative .v files, testbench included" },
      },
      required: ["sources"],
    },
    requires: "iverilog",
    install: { dnf: "iverilog", apt: "iverilog", brew: "icarus-verilog" },
    async run(input, ctx) {
      const sources = input.sources as string[];
      sources.forEach((s) => projectPath(ctx.projectDir, s));
      await fs.mkdir(projectPath(ctx.projectDir, "build"), { recursive: true });
      const compiled = await run("iverilog", ["-g2012", "-o", "build/sim.vvp", ...sources], ctx.projectDir);
      if (compiled.code !== 0) throw new ToolError(report(compiled));
      return { text: report(await run("vvp", ["build/sim.vvp"], ctx.projectDir, 60_000)) };
    },
  },
  {
    domain: "chip",
    name: "chip_synthesize",
    description:
      "Synthesize Verilog to a generic gate netlist with Yosys and report cell counts. Writes build/<top>.json. The first step towards place-and-route with OpenROAD/OpenLane.",
    input_schema: {
      type: "object",
      properties: {
        sources: { type: "array", items: { type: "string" }, minItems: 1, description: "Project-relative .v files (no testbench)" },
        top: { type: "string", description: "Top module name" },
      },
      required: ["sources", "top"],
    },
    requires: "yosys",
    install: { dnf: "yosys", apt: "yosys", brew: "yosys" },
    async run(input, ctx) {
      const top = input.top as string;
      if (!MODULE.test(top)) throw new ToolError("top must be a Verilog identifier");
      const sources = input.sources as string[];
      sources.forEach((s) => projectPath(ctx.projectDir, s));
      const script = `${sources.map((s) => `read_verilog -sv ${JSON.stringify(s)}`).join("; ")}; synth -top ${top}; stat; write_json build/${top}.json`;
      await fs.mkdir(projectPath(ctx.projectDir, "build"), { recursive: true });
      const r = await run("yosys", ["-p", script], ctx.projectDir);
      if (r.code !== 0) throw new ToolError(report(r));
      return { text: report(r), files: [`build/${top}.json`] };
    },
  },
];
