import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { findProgram } from "../server/tools/exec.ts";
import { freecadTools } from "../server/tools/freecad.ts";
import { resolveApp } from "../server/tools/launch.ts";
import { FREECAD, script3dTools } from "../server/tools/script3d.ts";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const hasFreecad = !!resolveApp(FREECAD);
const skip = !hasFreecad && "freecadcmd not installed";
const tool = (name: string) => [...script3dTools, ...freecadTools].find((t) => t.name === name)!;

async function project(files: Record<string, string>) {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "forge-fc-"));
  for (const [rel, text] of Object.entries(files)) {
    await fs.mkdir(path.dirname(path.join(dir, rel)), { recursive: true });
    await fs.writeFile(path.join(dir, rel), text);
  }
  return dir;
}

// A 60 × 40 × 8 plate using every kind of standard hole from the forge helpers.
const PLATE = `
p = forge.rounded_box(60, 40, 8, radius=4)
p = forge.hole(p, 6, 6, "M3", kind="counterbore")
p = forge.hole(p, 54, 6, "M4", kind="countersink")
p = forge.hole(p, 54, 34, "M3", kind="tap", depth=6)
p = forge.hole(p, 6, 34, "M3", kind="insert")
result = forge.hole(p, 30, 20, "M5")
`;

test("inputs are checked before FreeCAD starts", async () => {
  const dir = await project({ "models/a.stl": "solid x\nendsolid x\n", "models/b.step": "x" });
  await assert.rejects(tool("freecad_inspect").run({ models: ["models/a.stl"] }, { projectDir: dir }), /STEP, BREP, IGES or FCStd/);
  await assert.rejects(tool("freecad_inspect").run({ models: ["models/none.step"] }, { projectDir: dir }), /does not exist/);
  await assert.rejects(tool("freecad_inspect").run({ models: ["models/b.step"], material: "cheese" }, { projectDir: dir }), /unknown material/);
  await assert.rejects(tool("freecad_inspect").run({ models: ["../b.step"] }, { projectDir: dir }), /outside the project/);
  await assert.rejects(tool("freecad_drawing").run({ model: "models/b.step", name: "a b" }, { projectDir: dir }), /name may only/);
});

test("forge helpers cut ISO holes, and inspection identifies each one with mass and print data", { skip, timeout: 120_000 }, async () => {
  const dir = await project({ "cad/plate.py": PLATE });
  const built = await tool("freecad_build").run({ script: "cad/plate.py", name: "plate", formats: ["step"] }, { projectDir: dir });
  assert.match(built.text, /60 × 40 × 8 mm.*valid: true/);

  const out = await tool("freecad_inspect").run({ models: ["models/plate.step"], material: "aluminium 6061" }, { projectDir: dir });
  assert.match(out.text, /5 hole\(s\)/);
  assert.match(out.text, /Ø3\.4 through along Z at \(6, 6, 8\), counterbore Ø6\.5 × 3\.3 \[M3 clearance\]/);
  assert.match(out.text, /Ø4\.5 through .*countersink Ø8\.96 \[M4 clearance\]/);
  assert.match(out.text, /Ø2\.5 6 deep along Z at \(54, 34, 8\) \[M3 tap\]/);
  assert.match(out.text, /Ø4 6\.2 deep .*\[M3 insert\]/);
  assert.match(out.text, /Ø5\.5 through .*\[M5 clearance\]/);
  const volume = Number(/volume ([\d.]+) mm³/.exec(out.text)![1]);
  const mass = Number(/mass ([\d.]+) g in Aluminium 6061/.exec(out.text)![1]);
  assert.ok(Math.abs(mass - (volume / 1000) * 2.7) < 0.01);
  assert.match(out.text, /0 mm² overhangs steeper than 45°/);
});

test("rounded pocket corners and edge chamfers are not holes, and blind 3.6 mm holes are M2.5 inserts", { skip, timeout: 120_000 }, async () => {
  const dir = await project({
    "cad/box.py": `
box = forge.enclosure(90, 60, 32, wall=2.4, floor=2.4, radius=5)
box = forge.standoffs(box, forge.grid(12.5, 15, 65, 30), "M2.5", height=5, base=2.4)
box = forge.hole(box, 45, 30, "M3", top=2.4)
result = forge.chamfer(box, 0.4, forge.edges_at(box, 0))
`,
  });
  await tool("freecad_build").run({ script: "cad/box.py", name: "box", formats: ["step"] }, { projectDir: dir });
  const out = await tool("freecad_inspect").run({ models: ["models/box.step"] }, { projectDir: dir });
  assert.match(out.text, /5 hole\(s\)/);
  assert.equal((out.text.match(/Ø3\.6 5 deep along Z .*\[M2\.5 insert\]/g) ?? []).length, 4, out.text);
  assert.match(out.text, /Ø3\.4 through along Z at \(45, 30, 2\.4\) \[M3 clearance\]/);
  assert.doesNotMatch(out.text, /countersink/);
});

test("inspection finds interference and clearance between the parts of an assembly", { skip, timeout: 120_000 }, async () => {
  const dir = await project({
    "cad/asm.py": `
a = doc.addObject("Part::Feature", "A"); a.Shape = Part.makeBox(10, 10, 10)
b = doc.addObject("Part::Feature", "B"); b.Shape = Part.makeBox(10, 10, 10, Vector(8, 0, 0))
c = doc.addObject("Part::Feature", "C"); c.Shape = Part.makeBox(10, 10, 10, Vector(0, 11.5, 0))
`,
  });
  await tool("freecad_build").run({ script: "cad/asm.py", name: "asm", formats: ["step"] }, { projectDir: dir });
  const out = await tool("freecad_inspect").run({ models: ["models/asm.step"] }, { projectDir: dir });
  assert.match(out.text, /asm\.step#1 and asm\.step#2: INTERFERE by 200 mm³/);
  assert.match(out.text, /asm\.step#1 and asm\.step#3: clearance 1\.5 mm/);
});

test("a drawing has the views, dimensions, hole table and title block, plus DXF and PDF", { skip, timeout: 180_000 }, async () => {
  const dir = await project({ "cad/plate.py": PLATE });
  await tool("freecad_build").run({ script: "cad/plate.py", name: "plate", formats: ["step"] }, { projectDir: dir });
  const out = await tool("freecad_drawing").run(
    { model: "models/plate.step", name: "plate", title: "Mounting plate", material: "PLA", dxf: true },
    { projectDir: dir },
  );
  assert.match(out.text, /Drawing on A3 at 2:1, first angle projection, part 60 × 40 × 8 mm/);
  assert.match(out.text, /H1: Ø3\.4 THRU {2}CBORE Ø6\.5 ↧3\.3, axis Z, X 6 {2}Y 6 \(M3 clearance\)/);
  assert.match(out.text, /H5: Ø2\.5 ↧6, axis Z, X 54 {2}Y 34 \(M3 tap\)/);
  const svg = await fs.readFile(path.join(dir, "drawings/plate.svg"), "utf8");
  assert.match(svg, /^<svg xmlns="http:\/\/www\.w3\.org\/2000\/svg" width="420mm" height="297mm"/);
  for (const text of ["FRONT", "TOP", "LEFT", "ISOMETRIC", "Mounting plate", "PLA", "ISO 2768-m", "2:1", ">60<", ">40<", ">8<", "H5", "M3 tap"]) {
    assert.ok(svg.includes(text), `drawing lacks ${text}`);
  }
  assert.ok((svg.match(/class="hid"/g) ?? []).length > 4, "hidden lines are drawn");

  const dxf = await fs.readFile(path.join(dir, "drawings/plate-top.dxf"), "utf8");
  assert.match(dxf, /^0\nSECTION\n2\nENTITIES\n/);
  assert.match(dxf, /0\nEOF\n$/);
  assert.equal((dxf.match(/\nCIRCLE\n/g) ?? []).length + (dxf.match(/\nARC\n/g) ?? []).length >= 9, true);

  if (findProgram(["rsvg-convert"])) {
    assert.ok(out.files!.includes("drawings/plate.pdf"));
    assert.equal((await fs.readFile(path.join(dir, "drawings/plate.pdf"))).subarray(0, 5).toString(), "%PDF-");
  } else {
    assert.match(out.text, /librsvg2-tools/);
  }

  const third = await tool("freecad_drawing").run({ model: "models/plate.step", name: "plate-us", projection: "third", sheet: "A3" }, { projectDir: dir });
  assert.match(third.text, /Drawing on A3 at \d+:1, third angle projection/);
  assert.ok((await fs.readFile(path.join(dir, "drawings/plate-us.svg"), "utf8")).includes("RIGHT"));
});

test("PartDesign bodies built from constrained sketches export as the part", { skip, timeout: 120_000 }, async () => {
  const dir = await project({
    "cad/pd.py": `
import Sketcher
body = doc.addObject("PartDesign::Body", "Body")
sk = body.newObject("Sketcher::SketchObject", "Profile")
sk.AttachmentSupport = (doc.getObject("XY_Plane"), [""])
sk.MapMode = "FlatFace"
pts = [Vector(0, 0, 0), Vector(30, 0, 0), Vector(30, 20, 0), Vector(0, 20, 0)]
for i in range(4):
    sk.addGeometry(Part.LineSegment(pts[i], pts[(i + 1) % 4]))
for i in range(4):
    sk.addConstraint(Sketcher.Constraint("Coincident", i, 2, (i + 1) % 4, 1))
sk.addConstraint(Sketcher.Constraint("DistanceX", 0, 1, 0, 2, 30))
sk.addConstraint(Sketcher.Constraint("DistanceY", 1, 1, 1, 2, 20))
pad = body.newObject("PartDesign::Pad", "Pad")
pad.Profile = sk
pad.Length = 5
`,
  });
  const out = await tool("freecad_build").run({ script: "cad/pd.py", name: "pd", formats: ["step", "fcstd"] }, { projectDir: dir });
  assert.match(out.text, /1 solid\(s\): 30 × 20 × 5 mm, volume 3000 mm³, valid: true/);
});

test("project files open in FreeCAD, sandboxed to the project when it is a Flatpak", { timeout: 30_000 }, async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "forge-open-"));
  const log = path.join(dir, "opened.log");
  const stub = path.join(dir, "freecad-stub");
  await fs.writeFile(stub, `#!/bin/sh\necho "$@" >> "${log}"\n`, { mode: 0o755 });
  const proj = path.join(dir, "proj");
  await fs.mkdir(path.join(proj, "models"), { recursive: true });
  await fs.writeFile(path.join(proj, "models", "part.FCStd"), "x");

  // A child process, so the stub is the FreeCAD that open.ts finds when it loads.
  const child = spawnSync(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `const { openInApp, openerTypes } = await import(${JSON.stringify(path.join(ROOT, "server/open.ts"))});
       const proj = ${JSON.stringify(proj)};
       const types = openerTypes();
       const app = await openInApp(proj, "models/part.FCStd");
       const errors = [];
       for (const rel of ["models/x.txt", "../etc/passwd.step", "models/none.step"]) {
         await openInApp(proj, rel).catch((e) => errors.push(e.status));
       }
       console.log(JSON.stringify({ app, fcstd: types[".fcstd"], step: types[".step"], errors }));`,
    ],
    { encoding: "utf8", env: { ...process.env, FORGE_FREECAD_GUI: stub, FORGE_NO_FLATPAK: "1" } },
  );
  assert.equal(child.status, 0, child.stderr);
  assert.deepEqual(JSON.parse(child.stdout), { app: "FreeCAD", fcstd: "FreeCAD", step: "FreeCAD", errors: [400, 400, 404] });
  for (let i = 0; i < 50 && !(await fs.stat(log).catch(() => null)); i++) await new Promise((r) => setTimeout(r, 100));
  assert.equal((await fs.readFile(log, "utf8")).trim(), path.join(proj, "models", "part.FCStd"));
});
