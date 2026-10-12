import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { findProgram } from "../server/tools/exec.ts";
import { parseRunnerOutput, script3dTools } from "../server/tools/script3d.ts";

const tool = (name: string) => script3dTools.find((t) => t.name === name)!;
const hasBlender = !!findProgram(tool("blender_build").requires);
const hasFreecad = !!findProgram(tool("freecad_build").requires);

async function project(files: Record<string, string>) {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "forge-3d-"));
  for (const [rel, text] of Object.entries(files)) {
    await fs.mkdir(path.dirname(path.join(dir, rel)), { recursive: true });
    await fs.writeFile(path.join(dir, rel), text);
  }
  return dir;
}

test("runner output parsing finds the result, the error, or explains the crash", () => {
  assert.deepEqual(parseRunnerOutput({ code: 0, stdout: 'noise\nFORGE_RESULT {"a":1}\n', stderr: "" }), { result: { a: 1 } });
  assert.deepEqual(parseRunnerOutput({ code: 1, stdout: 'FORGE_ERROR "bad"\n', stderr: "" }), { error: "bad" });
  assert.match(parseRunnerOutput({ code: 139, stdout: "x", stderr: "segfault" }).error!, /code 139[\s\S]*segfault/);
});

test("3D script tools refuse missing scripts and bad names before launching anything", async () => {
  const dir = await project({});
  await assert.rejects(tool("freecad_build").run({ script: "cad/none.py", name: "x" }, { projectDir: dir }), /does not exist/);
  await assert.rejects(tool("blender_build").run({ script: "a.py", name: "../x" }, { projectDir: dir }), /name may only/);
});

test("FreeCAD builds a drilled plate and exports STEP, STL and FCStd", { skip: !hasFreecad && "freecadcmd not installed" }, async () => {
  const dir = await project({
    "cad/plate.py": `
plate = Part.makeBox(60, 40, 5)
for x, y in [(5, 5), (55, 5), (5, 35), (55, 35)]:
    plate = plate.cut(Part.makeCylinder(1.6, 5, App.Vector(x, y, 0)))
result = plate
`,
  });
  const out = await tool("freecad_build").run({ script: "cad/plate.py", name: "plate" }, { projectDir: dir });
  assert.match(out.text, /60 × 40 × 5 mm/);
  assert.match(out.text, /valid: true/);
  const volume = 60 * 40 * 5 - 4 * Math.PI * 1.6 ** 2 * 5;
  assert.ok(out.text.includes(`volume ${Number(volume.toFixed(3))}`) || Math.abs(Number(/volume ([\d.]+)/.exec(out.text)![1]) - volume) < 0.01);
  assert.deepEqual(out.files, ["models/plate.step", "models/plate.stl", "models/plate.FCStd"]);
  assert.match(await fs.readFile(path.join(dir, "models/plate.step"), "utf8"), /^ISO-10303-21;/);
  assert.ok((await fs.stat(path.join(dir, "models/plate.stl"))).size > 1000);
});

test("FreeCAD reports script errors to the agent", { skip: !hasFreecad && "freecadcmd not installed" }, async () => {
  const dir = await project({ "cad/bad.py": "result = Part.makeBox(1, 1)\n" });
  await assert.rejects(tool("freecad_build").run({ script: "cad/bad.py", name: "bad" }, { projectDir: dir }), /FreeCAD: the script raised an error/);
});

test("Blender builds, exports and renders a scene", { skip: !hasBlender && "blender not installed", timeout: 600_000 }, async () => {
  const dir = await project({
    "art/scene.py": `
import bpy
bpy.ops.mesh.primitive_uv_sphere_add(radius=1, location=(0, 0, 1))
ball = bpy.context.object
ball.modifiers.new("smooth", "SUBSURF").levels = 2
mat = bpy.data.materials.new("glossy")
mat.use_nodes = True
mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.8, 0.2, 0.1, 1)
ball.data.materials.append(mat)
bpy.ops.mesh.primitive_plane_add(size=6)
`,
  });
  const out = await tool("blender_build").run({ script: "art/scene.py", name: "ball", formats: ["glb", "stl"], render: true }, { projectDir: dir });
  assert.match(out.text, /Blender built 2 objects/);
  assert.deepEqual(out.files, ["models/ball.glb", "models/ball.stl", "renders/ball.png"]);
  const glb = await fs.readFile(path.join(dir, "models/ball.glb"));
  assert.equal(glb.subarray(0, 4).toString(), "glTF");
  const png = await fs.readFile(path.join(dir, "renders/ball.png"));
  assert.equal(png.subarray(1, 4).toString(), "PNG");
});

test("Blender reports script errors to the agent", { skip: !hasBlender && "blender not installed" }, async () => {
  const dir = await project({ "art/bad.py": "import bpy\nbpy.ops.mesh.primitive_no_such_thing()\n" });
  await assert.rejects(tool("blender_build").run({ script: "art/bad.py", name: "bad" }, { projectDir: dir }), /Blender: the script raised an error/);
});
