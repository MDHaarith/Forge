import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { boardTools, pcbEngine, summarizeWarnings } from "../server/tools/board.ts";
import { designTools, type Design } from "../server/tools/design.ts";
import { apply, fitCheck, parseStl, rotationMatrix, triangleHitsBox, unapply, type BoardBoxes, type Vec } from "../server/tools/fit.ts";

const assemble = designTools.find((t) => t.name === "design_assemble")!;
const build = boardTools.find((t) => t.name === "pcb_build")!;

async function project(files: Record<string, string | Buffer>) {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "forge-pcb-"));
  for (const [rel, data] of Object.entries(files)) {
    await fs.mkdir(path.dirname(path.join(dir, rel)), { recursive: true });
    await fs.writeFile(path.join(dir, rel), data);
  }
  return dir;
}

/** The 12 triangles of an axis-aligned box. */
function boxTriangles(min: Vec, max: Vec): number[] {
  const c = (i: number): number[] => [0, 1, 2].map((k) => ((i >> k) & 1 ? max[k] : min[k]));
  const faces = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
  return faces.flatMap(([a, b, d, e]) => [...c(a), ...c(b), ...c(d), ...c(a), ...c(d), ...c(e)]);
}

function binaryStl(numbers: number[]): Buffer {
  const count = numbers.length / 9;
  const buf = Buffer.alloc(84 + count * 50);
  buf.writeUInt32LE(count, 80);
  for (let i = 0; i < count; i++) for (let k = 0; k < 9; k++) buf.writeFloatLE(numbers[i * 9 + k], 84 + i * 50 + 12 + k * 4);
  return buf;
}

function asciiStl(numbers: number[]): string {
  const lines = ["solid t"];
  for (let i = 0; i < numbers.length; i += 9) {
    lines.push(" facet normal 0 0 0", "  outer loop");
    for (let k = 0; k < 9; k += 3) lines.push(`   vertex ${numbers[i + k]} ${numbers[i + k + 1]} ${numbers[i + k + 2]}`);
    lines.push("  endloop", " endfacet");
  }
  return lines.join("\n") + "\nendsolid t\n";
}

// A 40 × 30 × 1.6 mm board with a 5 mm tall connector J1 near its +x edge.
const board: BoardBoxes = { size: [40, 30, 1.6], center: [0, 0], components: [{ name: "J1", layer: "top", min: [14, -3, 0.8], max: [19, 3, 5.8] }] };
const place = (position: Vec = [0, 0, 0], rotation: Vec = [0, 0, 0]) => ({ position, rotation });

test("STL files parse whether binary or ASCII", () => {
  const tris = boxTriangles([0, 0, 0], [1, 2, 3]);
  assert.deepEqual(Array.from(parseStl(binaryStl(tris))), tris);
  assert.deepEqual(Array.from(parseStl(Buffer.from(asciiStl(tris)))), tris);
  assert.throws(() => parseStl(Buffer.from("hello")), /not an STL/);
});

test("rotations follow the 3D view's XYZ order and undo exactly", () => {
  const m = rotationMatrix([0, 0, 90]);
  apply(m, [0, 0, 0], [1, 0, 0]).forEach((v, i) => assert.ok(Math.abs(v - [0, 1, 0][i]) < 1e-9));
  const r = rotationMatrix([30, -45, 120]);
  const p: Vec = [3, -7, 11];
  unapply(r, [1, 2, 3], apply(r, [1, 2, 3], p)).forEach((v, i) => assert.ok(Math.abs(v - p[i]) < 1e-9));
});

test("a triangle hits a box only when they overlap", () => {
  assert.equal(triangleHitsBox([0, 0, 0], [1, 1, 1], [-5, -5, 0], [5, -5, 0], [0, 5, 0]), true);
  assert.equal(triangleHitsBox([0, 0, 0], [1, 1, 1], [-5, -5, 2], [5, -5, 2], [0, 5, 2]), false);
  // Its bounding box overlaps the box, but the slanted face passes by the corner.
  assert.equal(triangleHitsBox([0, 0, 0], [1, 1, 1], [1.5, 3, 0], [3, 1.5, 0], [3, 3, 0]), false);
});

test("the fit check finds collisions, tight spots and clear fits", () => {
  const wall = (x: number, bottom = -5) => ({ name: "wall.stl", placement: place(), triangles: Float32Array.from(boxTriangles([x, -20, bottom], [x + 2, 20, 10])) });
  const boards = [{ name: "main.glb", placement: place(), board }];
  // A rib hanging down to 1.5 mm cuts through J1 (x 14..19) but clears the board's top (0.8).
  assert.deepEqual(fitCheck(boards, [wall(17, 1.5)], 0.5), [{ board: "main.glb", item: "J1", part: "wall.stl", status: "collision" }]);
  assert.deepEqual(fitCheck(boards, [wall(20.3)], 0.5), [{ board: "main.glb", item: "board", part: "wall.stl", status: "tight" }]);
  assert.deepEqual(fitCheck(boards, [wall(25)], 0.5), []);
  // Turned 180° about Z, J1 sits at the -x edge instead.
  assert.deepEqual(fitCheck([{ ...boards[0], placement: place([0, 0, 0], [0, 0, 180]) }], [wall(17, 1.5)], 0.5), []);
  // A board resting on a standoff only touches it.
  const standoff = { name: "post.stl", placement: place(), triangles: Float32Array.from(boxTriangles([-18, -13, -5], [-15, -10, -0.8])) };
  assert.deepEqual(fitCheck(boards, [standoff], 0.5), [{ board: "main.glb", item: "board", part: "post.stl", status: "tight" }]);
});

test("design_assemble writes the assembly and reports the fit", async () => {
  const dir = await project({
    "pcb/main.glb": "glb",
    "pcb/main.board.json": JSON.stringify(board),
    "models/lid.stl": binaryStl(boxTriangles([-25, -20, 4], [25, 20, 6])),
    "models/lid.step": "ISO-10303-21;",
  });
  const low = await assemble.run({ name: "low", parts: [{ file: "pcb/main.glb" }, { file: "models/lid.step", color: "#8899aa", opacity: 0.4 }] }, { projectDir: dir });
  assert.match(low.text, /1 COLLISION/);
  assert.match(low.text, /J1 on pcb\/main\.glb cuts into models\/lid\.stl/);
  assert.match(low.text, /tallest part J1 reaches 5 mm above the board/);
  assert.deepEqual(low.files, ["design/low.json"]);
  const saved: Design = JSON.parse(await fs.readFile(path.join(dir, "design/low.json"), "utf8"));
  assert.deepEqual(saved.parts[1], { file: "models/lid.stl", position: [0, 0, 0], rotation: [0, 0, 0], color: "#8899aa", opacity: 0.4 });
  assert.equal(saved.fit[0].status, "collision");

  const high = await assemble.run({ name: "high", parts: [{ file: "pcb/main.glb" }, { file: "models/lid.stl", position: [0, 0, 3] }] }, { projectDir: dir });
  assert.match(high.text, /everything clears by at least 0\.5 mm/);
  assert.match(high.text, /models\/lid\.stl spans x -25, 25, y -20, 20, z 7, 9 mm/);
});

test("design_assemble refuses bad input", async () => {
  const dir = await project({ "models/a.stl": binaryStl(boxTriangles([0, 0, 0], [1, 1, 1])), "notes.txt": "x" });
  const run = (input: Record<string, unknown>) => assemble.run(input, { projectDir: dir });
  await assert.rejects(run({ name: "a b", parts: [{ file: "models/a.stl" }] }), /name may only/);
  await assert.rejects(run({ name: "a", parts: [] }), /1 to 50 parts/);
  await assert.rejects(run({ name: "a", parts: [{ file: "notes.txt" }] }), /must be \.stl or \.glb/);
  await assert.rejects(run({ name: "a", parts: [{ file: "pcb/none.glb" }] }), /does not exist; build it first/);
  await assert.rejects(run({ name: "a", parts: [{ file: "../a.stl" }] }), /outside the project/);
  await assert.rejects(run({ name: "a", parts: [{ file: "models/a.stl", position: [1, 2] }] }), /position must be three numbers/);
  await assert.rejects(run({ name: "a", parts: [{ file: "models/a.stl", color: "red" }] }), /color must look like/);
  await assert.rejects(run({ name: "a", parts: [{ file: "models/a.stl", opacity: 0 }] }), /opacity must be/);
  await assert.rejects(run({ name: "a", parts: [{ file: "models/a.stl" }], clearance: -1 }), /clearance must be/);
});

test("engine warnings are grouped and the routine ones dropped", () => {
  const lines = summarizeWarnings(
    [
      "source_unnamed_trace_warning: trace has no name",
      "source_missing_manufacturer_part_number_warning: R1 has none",
      "source_missing_manufacturer_part_number_warning: R2 has none",
      "source_part_not_found_warning: R1 not found",
      "pcb_silkscreen_overlap_warning: R1 label overlaps C1",
      "pcb_silkscreen_overlap_warning: R2 label overlaps C2",
    ],
    false,
  );
  assert.equal(lines.length, 2);
  assert.match(lines[0], /^2 part\(s\) have no manufacturer part number.*lookup_parts: true/);
  assert.equal(lines[1], "pcb_silkscreen_overlap_warning ×2, e.g. R1 label overlaps C1");
  assert.match(summarizeWarnings(["source_part_not_found_warning: R1 not found"], true)[0], /R1 not found/);
});

const noEngine = !pcbEngine() && "PCB engine not built (npm run pcb-engine)";

const LED_BOARD = `export default () => (
  <board width="30mm" height="20mm">
    <resistor name="R1" resistance="1k" footprint="0402" pcbX={-6} pcbY={0} />
    <led name="LED1" color="red" footprint="0603" pcbX={6} pcbY={0} />
    <pinheader name="J1" pinCount={2} pitch="2.54mm" pcbX={0} pcbY={-6} />
    <trace from=".J1 > .pin1" to=".R1 > .pin1" />
    <trace from=".R1 > .pin2" to=".LED1 > .anode" />
    <trace from=".LED1 > .cathode" to=".J1 > .pin2" />
  </board>
)
`;

test("pcb_build routes a board and writes its views, 3D model and fab files", { skip: noEngine, timeout: 120_000 }, async () => {
  const dir = await project({ "pcb/led.tsx": LED_BOARD });
  const out = await build.run({ source: "pcb/led.tsx" }, { projectDir: dir });
  assert.match(out.text, /board 30 × 20 mm/);
  assert.match(out.text, /3 part\(s\): R1, LED1, J1/);
  assert.match(out.text, /No errors: every connection is routed/);
  for (const f of ["pcb/led-pcb.svg", "pcb/led-schematic.svg", "pcb/led.glb", "pcb/led.circuit.json", "pcb/led.board.json", "fab/led/"]) assert.ok(out.files?.includes(f), f);
  const data: BoardBoxes = JSON.parse(await fs.readFile(path.join(dir, "pcb/led.board.json"), "utf8"));
  assert.deepEqual(data.size.slice(0, 2), [30, 20]);
  assert.deepEqual(data.components.map((c) => c.name).sort(), ["J1", "LED1", "R1"]);
  const fab = await fs.readdir(path.join(dir, "fab/led"));
  assert.ok(fab.some((f) => f.endsWith(".gbr")) && fab.includes("led-PTH.drl") && fab.includes("led-pnp.csv"), fab.join(", "));
  const bom = await fs.readFile(path.join(dir, "fab/led/led-bom.csv"), "utf8");
  assert.match(bom, /^Comment,Designator,Footprint,LCSC Part #/);
  assert.match(bom, /1k.*R1/);
  assert.equal((await fs.readFile(path.join(dir, "pcb/led.glb"))).subarray(0, 4).toString(), "glTF");
});

test("pcb_build pours a ground plane", { skip: noEngine, timeout: 120_000 }, async () => {
  const dir = await project({
    "pcb/gnd.tsx": LED_BOARD.replace("</board>", `  <trace from=".J1 > .pin2" to="net.GND" />\n    <copperpour connectsTo="net.GND" layer="bottom" />\n  </board>`),
  });
  const out = await build.run({ source: "pcb/gnd.tsx" }, { projectDir: dir });
  assert.match(out.text, /Copper pours: GND on bottom/);
  assert.doesNotMatch(out.text, /engine_step_error/);
  assert.match(await fs.readFile(path.join(dir, "fab/gnd/gnd-B_Cu.gbr"), "utf8"), /G36/);
});

test("pcb_build accepts a printed antenna's open arm and clears ground under a keepout", { skip: noEngine, timeout: 120_000 }, async () => {
  const dir = await project({
    "pcb/ant.tsx": `export default () => (
  <board width="50mm" height="40mm">
    <antenna name="ANT1" antennaShape="2.4ghz_quarter_wave_monopole" pcbX={-20} pcbY={18.4} />
    <keepout pcbX={-3} pcbY={16} width="32mm" height="8mm" shape="rect" layers={["bottom"]} />
    <copperpour connectsTo="net.GND" layer="bottom" />
  </board>
)`,
  });
  const out = await build.run({ source: "pcb/ant.tsx" }, { projectDir: dir });
  assert.match(out.text, /No errors/);
  assert.doesNotMatch(out.text, /disconnected endpoint/);
  const circuit = JSON.parse(await fs.readFile(path.join(dir, "pcb/ant.circuit.json"), "utf8"));
  const pour = circuit.find((e: { type: string }) => e.type === "pcb_copper_pour");
  const ys = pour.brep_shape.outer_ring.vertices.filter((v: { x: number }) => v.x > -15 && v.x < 10).map((v: { y: number }) => v.y);
  assert.ok(Math.max(...ys) <= 12.1, `ground reaches y ${Math.max(...ys)} under the antenna`);
});

test("pcb_build reports board errors and keeps the board code sandboxed", { skip: noEngine, timeout: 120_000 }, async () => {
  const outside = await fs.mkdtemp(path.join(os.tmpdir(), "forge-outside-"));
  process.env.FORGE_TEST_SECRET = "s3cret";
  const dir = await project({
    "pcb/env.tsx": `throw new Error("KEY=" + (process.env.FORGE_TEST_SECRET ?? "hidden"))\nexport default () => <board width="10mm" height="10mm" />\n`,
    "pcb/escape.tsx": `const fs = process.getBuiltinModule("node:fs")\nlet r = "written"\ntry { fs.writeFileSync(${JSON.stringify(path.join(outside, "x.txt"))}, "x") } catch { r = "blocked" }\nthrow new Error("WRITE=" + r)\nexport default () => <board width="10mm" height="10mm" />\n`,
  });
  try {
    await assert.rejects(build.run({ source: "pcb/env.tsx" }, { projectDir: dir }), /KEY=hidden/);
    await assert.rejects(build.run({ source: "pcb/escape.tsx" }, { projectDir: dir }), /WRITE=blocked/);
    assert.deepEqual(await fs.readdir(outside), []);
  } finally {
    delete process.env.FORGE_TEST_SECRET;
  }
  await assert.rejects(build.run({ source: "pcb/none.tsx" }, { projectDir: dir }), /does not exist/);
  await assert.rejects(build.run({ source: "pcb/env.tsx", name: "a/b" }, { projectDir: dir }), /name may only/);
});
