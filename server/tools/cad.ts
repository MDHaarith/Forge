import fs from "node:fs/promises";
import path from "node:path";
import { bounds, partMesh, toAsciiStl, toScad, type Part } from "../cad/mesh.ts";
import { report, run } from "./exec.ts";
import { projectPath } from "./paths.ts";
import { ToolError, type ForgeTool } from "./types.ts";

const vec3 = { type: "array", items: { type: "number" }, minItems: 3, maxItems: 3 };

const NAME = /^[A-Za-z0-9_-]{1,64}$/;

export const cadTools: ForgeTool[] = [
  {
    domain: "3d",
    name: "cad_model",
    description:
      "Build a 3D part from primitive shapes, in millimetres, Z up. Writes models/<name>.stl (printable, shown in the 3D viewer), models/<name>.scad (editable OpenSCAD source) and models/<name>.json (the part list). Shapes are placed side by side; overlapping shapes merge when sliced. box: `size` [x,y,z] with its minimum corner at `at`. cylinder: `radius`, `height`, base centre at `at`. sphere: `radius`, centre at `at`.",
    input_schema: {
      type: "object",
      properties: {
        name: { type: "string", description: "File name without extension: letters, digits, - and _" },
        parts: {
          type: "array",
          minItems: 1,
          items: {
            type: "object",
            properties: {
              shape: { type: "string", enum: ["box", "cylinder", "sphere"] },
              size: vec3,
              radius: { type: "number" },
              height: { type: "number" },
              at: vec3,
            },
            required: ["shape"],
          },
        },
      },
      required: ["name", "parts"],
    },
    async run(input, ctx) {
      const name = input.name as string;
      if (!NAME.test(name)) throw new ToolError("name may only use letters, digits, - and _");
      const parts = input.parts as Part[];
      parts.forEach((p, i) => {
        const missing =
          p.shape === "box" ? (p.size ? null : "size")
          : p.shape === "cylinder" ? (p.radius && p.height ? null : "radius and height")
          : p.radius ? null : "radius";
        if (missing) throw new ToolError(`parts[${i}] (${p.shape}) needs ${missing}`);
      });
      const tris = parts.flatMap(partMesh);
      const dir = projectPath(ctx.projectDir, "models");
      await fs.mkdir(dir, { recursive: true });
      await fs.writeFile(path.join(dir, `${name}.stl`), toAsciiStl(name, tris));
      await fs.writeFile(path.join(dir, `${name}.scad`), toScad(parts));
      await fs.writeFile(path.join(dir, `${name}.json`), JSON.stringify({ name, units: "mm", parts }, null, 2) + "\n");
      const { min, max } = bounds(tris);
      const size = max.map((v, i) => +(v - min[i]).toFixed(2));
      return {
        text: `built ${name}: ${parts.length} parts, ${tris.length} triangles, overall ${size.join(" x ")} mm`,
        files: [`models/${name}.stl`, `models/${name}.scad`, `models/${name}.json`],
      };
    },
  },
  {
    domain: "3d",
    name: "openscad_render",
    description: "Render an OpenSCAD file in the project to STL with the real OpenSCAD engine (supports difference(), hull(), etc.).",
    input_schema: {
      type: "object",
      properties: {
        path: { type: "string", description: "Project-relative .scad file" },
        output: { type: "string", description: "Project-relative .stl to write (default: same name)" },
      },
      required: ["path"],
    },
    requires: "openscad",
    install: "https://openscad.org/downloads.html",
    async run(input, ctx) {
      const src = input.path as string;
      const out = (input.output as string | undefined) ?? src.replace(/\.scad$/i, "") + ".stl";
      projectPath(ctx.projectDir, src);
      projectPath(ctx.projectDir, out);
      const r = await run("openscad", ["-o", out, src], ctx.projectDir);
      if (r.code !== 0) throw new ToolError(report(r));
      return { text: `rendered ${out}\n${report(r)}`, files: [out] };
    },
  },
];
