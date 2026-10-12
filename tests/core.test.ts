import assert from "node:assert/strict";
import { test } from "node:test";
import { bounds, box, cylinder, sphere, toAsciiStl, toScad } from "../server/cad/mesh.ts";
import { projectPath } from "../server/tools/paths.ts";
import { validate } from "../server/tools/validate.ts";

test("box is 12 triangles spanning its size", () => {
  const tris = box([10, 20, 30], [1, 2, 3]);
  assert.equal(tris.length, 12);
  assert.deepEqual(bounds(tris), { min: [1, 2, 3], max: [11, 22, 33] });
});

test("cylinder and sphere stay within their radius", () => {
  const c = bounds(cylinder(5, 10, [0, 0, 2]));
  assert.ok(Math.abs(c.max[0] - 5) < 1e-9 && Math.abs(c.min[2] - 2) < 1e-9 && Math.abs(c.max[2] - 12) < 1e-9);
  const s = bounds(sphere(4));
  assert.ok(s.max.every((v) => v <= 4 + 1e-9) && s.min.every((v) => v >= -4 - 1e-9));
});

test("STL output is a closed ASCII solid", () => {
  const stl = toAsciiStl("part", box([1, 1, 1]));
  assert.match(stl, /^solid part\n/);
  assert.match(stl, /endsolid part\n$/);
  assert.equal(stl.match(/facet normal/g)?.length, 12);
  // The bottom face points down.
  assert.match(stl, /facet normal 0 0 -1/);
});

test("OpenSCAD output mirrors the parts", () => {
  const scad = toScad([{ shape: "box", size: [1, 2, 3], at: [4, 5, 6] }, { shape: "cylinder", radius: 2, height: 5 }]);
  assert.match(scad, /translate\(\[4, 5, 6\]\) cube\(\[1, 2, 3\]\);/);
  assert.match(scad, /cylinder\(r = 2, h = 5/);
});

test("project paths cannot escape the project or touch .git", () => {
  assert.equal(projectPath("/p", "a/b.txt"), "/p/a/b.txt");
  assert.throws(() => projectPath("/p", "../etc/passwd"), /outside the project/);
  assert.throws(() => projectPath("/p", "/etc/passwd"), /outside the project/);
  assert.throws(() => projectPath("/p", ".git/config"), /managed by the git tools/);
  assert.equal(projectPath("/p", ".github/x"), "/p/.github/x");
});

test("validate reports missing, mistyped and out-of-enum values", () => {
  const schema = {
    type: "object",
    properties: {
      name: { type: "string" },
      at: { type: "array", items: { type: "number" }, minItems: 3, maxItems: 3 },
      shape: { type: "string", enum: ["box"] },
    },
    required: ["name"],
  };
  assert.deepEqual(validate({ name: "x", at: [1, 2, 3], shape: "box" }, schema), []);
  assert.deepEqual(validate({}, schema), ["input.name is required"]);
  assert.deepEqual(validate({ name: 1, at: [1, "2"], shape: "cone" }, schema), [
    "input.name must be a string",
    "input.at needs at least 3 items",
    "input.at[1] must be a number",
    'input.shape must be one of "box"',
  ]);
  assert.deepEqual(validate("nope", schema), ["input must be an object"]);
});
