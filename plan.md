# Plan: Stage 1, Forge's own CAD kernel

Written 2026-10-13 for an AI session that has no memory of earlier chats.

**If you are the orchestrator (the session that reads this first):**
1. Read this whole file, then `AGENTS.md`, `docs/ARCHITECTURE.md` and `docs/ROADMAP.md`.
2. Find the first unticked step under "Work order".
3. Send that step's brief to the model named on it. Copy the brief exactly and add nothing.
4. Check the result with that step's "Orchestrator check".
5. Tick the box and commit. Then do the next step.

Do one step at a time. Never let two agents edit the checkout at once.

---

## 1. Where things stand

- Forge is at v0.11.0 on `main`. The Rust server is in `server/`, the Tauri desktop app in `desktop/`,
  and the web UI in `web/`.
- Goal: Forge must run with no FreeCAD, KiCad or Blender installed. A CAD kernel compiled into the
  Forge binary counts as Forge; a separately installed program does not.
- Today, real solids need FreeCAD:
  - `freecad_build`, `freecad_inspect` and `freecad_drawing` are in `server/src/tools/freecad.rs`.
  - `cad_model` (`server/src/tools/cad.rs`) is native but makes meshes only.
- `server/runners/forge_cad.py` already contains the engineering tables and the logic to port: hole
  sizes, materials, `hole`, `enclosure`, `lid`, `standoffs`, `place`, edge selectors, `holes_in`,
  `describe_diameter` and `overhangs`. Port them. Do not invent new numbers.

## 2. What stage 1 delivers

There are 11 features, reached by the AI through **two new tools**, `part_build` and `part_inspect`.
They will replace `freecad_build`, `freecad_inspect` and `cad_model`; `freecad_drawing` stays.

| # | Feature |
|---|---|
| 1 | Basic shapes: box, cylinder, sphere, cone |
| 2 | Extrude a 2D outline (lines, arcs, circle) |
| 3 | Revolve a 2D outline about the Z axis |
| 4 | Combine: union, cut, intersect |
| 5 | Fillet edges chosen by selector |
| 6 | Chamfer edges chosen by selector |
| 7 | Place: rotate then move |
| 8 | Standard holes M1.6–M12: clearance, tap, counterbore, countersink, insert |
| 9 | Enclosure, lid with lip, standoffs |
| 10 | Export STEP and STL; show in the 3D view |
| 11 | Inspect: size, volume, mass, holes, overhang |

**Finish line:** with FreeCAD uninstalled, ask Forge in the desktop app for "an enclosure for this board
with M3 standoffs and a lid". It builds the part, shows it, writes STEP and reports the mass.

**Not in stage 1:** constrained sketcher, feature-tree UI, loft, sweep, shell, threads, sheet metal,
drawings and a board editor.

**Decided:**
- The overhang warning uses 45° from vertical, the usual 3D-printing rule (the user confirmed this).
- Holes are found in STEP files only. An STL has no true circles.

## 3. The contract (every model follows this exactly)

All lengths are millimetres. All angles are degrees. Z is up.

### 3.1 `part_build` input

```json
{
  "name": "bracket",
  "material": "Aluminium 6061",
  "steps": [ { "op": "...", ... }, ... ]
}
```

- `name`: lowercase letters, digits, `-` and `_` only. `material` is optional and must be a key of
  `MATERIALS` in forge_cad.py; it is used for mass.
- Steps run in order. A shape step makes a **body**. `id` is optional; the default id is `s<index>`,
  where index counts from 0 over all steps.
- An operation step names its `target` body and changes it.
- `union`, `cut` and `intersect` also name a `tool` body, which is consumed (removed).
- When the steps end, every body still present is exported.

| op | fields | meaning |
|---|---|---|
| `box` | `size:[x,y,z]`, `at:[x,y,z]` (default 0,0,0) | box with its minimum corner at `at` |
| `cylinder` | `radius`, `height`, `at` | axis +Z, base centre at `at` |
| `sphere` | `radius`, `at` | centre at `at` |
| `cone` | `radius1` (bottom), `radius2` (top, may be 0), `height`, `at` | axis +Z, base centre at `at` |
| `extrude` | `outline`, `height`, `at` | outline in the XY plane, extruded +Z, then moved by `at` |
| `revolve` | `outline`, `angle` (default 360) | outline in the XZ plane (x = radius ≥ 0), turned about the Z axis, starting from +X towards +Y |
| `union` / `cut` / `intersect` | `target`, `tool` | boolean; an empty intersect is allowed and has volume 0 |
| `fillet` | `target`, `radius`, `edges` | round the edges |
| `chamfer` | `target`, `size`, `edges` | bevel the edges, equal distance |
| `place` | `target`, `rotate:[rx,ry,rz]`, `move:[x,y,z]` | rotate about X, then Y, then Z, through the origin; then move |
| `hole` | `target`, `points`, `size` ("M3"), `kind`, `fit` (default "normal"), `depth` (omit = through) | same rules as `hole()` in forge_cad.py |
| `enclosure` | `size:[x,y,z]`, `wall` (2), `floor` (2), `radius` (0) | same as `enclosure()` |
| `lid` | `size:[x,y]`, `thickness` (2), `wall` (2), `lip` (2), `gap` (0.2), `radius` (0), `at` | same as `lid()`: plate from z=0 to thickness, lip from z=-lip to 0, then moved by `at` |
| `standoffs` | `target`, `points`, `size`, `height` (5), `base` (0), `kind` (insert) | same as `standoffs()` |

- **`outline`** is either `{"circle":[cx, cy, r]}` or
  `{"start":[x,y], "segments":[ {"line":[x,y]} | {"arc":[x,y], "through":[x,y]} , ... ]}`.
  The last point must equal `start`.
- **`edges`** is `"all"`, `"vertical"` (straight edges parallel to Z), or `{"at_z": number}` (edges
  lying entirely at that height).
- **`points`** is `[[x,y], ...]`, or `{"grid":[x0,y0,dx,dy,nx,ny]}`, or
  `{"corners":[length,width,inset]}`. These follow `grid()` and `corners()`.
- **`kind`** is one of `"clearance"`, `"tap"`, `"counterbore"`, `"countersink"`, `"insert"`.

### 3.2 `part_build` output

It writes three files:
- `models/<name>.step`: all bodies;
- `models/<name>.stl`: all bodies, binary STL;
- `models/<name>.json`, which looks like this:

```json
{
  "name": "bracket",
  "material": "Aluminium 6061",
  "steps": [ ...the input steps, unchanged... ],
  "bodies": [
    { "id": "base", "volume": 3762.417, "bbox": [[0,0,0],[40,20,5]], "solids": 1 }
  ]
}
```

- `volume` is rounded to 3 decimals. `bbox` values are rounded to 3 decimals.
- The JSON uses `server/src/jsfmt.rs` number formatting and keeps key order.
- `ToolResult.files` lists the three paths. `ToolResult.text` is a short human summary.

Errors are `Err(ToolError)`, and their message must contain:
- an unknown op: `step <index>: unknown op "<op>"`;
- a missing body: `step <index>: no body "<id>"`;
- a fillet or chamfer that fails: `step <index>: fillet failed` (or `chamfer failed`).

### 3.3 `part_inspect` input and output

Input is `{"path":"models/x.step", "material":"PLA"}`. `material` is optional, and `path` may be a
`.step`, `.stp` or `.stl` file. `ToolResult.text` is exactly one JSON object, 2-space indented:

```json
{
  "file": "models/x.step",
  "size": [40, 20, 5],
  "volume": 3762.417,
  "material": "Aluminium 6061",
  "mass_g": 10.159,
  "holes": [ { "axis": "Z", "diameter": 5.5, "through": true, "depth": 5, "center": [8, 10, 5], "fits": "M5 clearance" } ],
  "overhang_area": 0,
  "overhang_warning": false
}
```

- `material` and `mass_g` are `null` when no material is given.
- For an STL, `holes` is the string `"unknown (mesh)"`.
- `overhang_area` is the downward-facing area steeper than 45° from vertical, excluding the lowest
  (bed) faces. It follows `overhangs()` in forge_cad.py.
- `overhang_warning` is `overhang_area > 1`.
- Holes follow `holes_in()` and `describe_diameter()`, with the same fields and sort order.

## 4. Models: who does what

Delegate with T3's `delegate_task`. Use standard service tier only, never "priority"/fast: fast mode
hit the Codex usage limit twice during the Rust port. If a usage limit is hit, wait for the reset. Do
not hand the step to another model.

| Role | provider / model / options |
|---|---|
| Orchestrator and reviewer (you) | `claudeAgent` / `claude-opus-5-5` / effort `high` |
| Kernel builder (steps 0, 2, 3, 4) | `codex` / `gpt-6.1-sol` / `{"reasoningEffort":"high","serviceTier":"default"}` |
| Test writer (step 1) | `antigravity` / `gemini-3.8-flash-high` |
| Tool descriptions (step 5) | `claudeAgent` / `claude-sonnet-5-5` / effort `medium` |

Gemini 3.8 Flash makes mistakes when a task is open-ended. Its brief therefore gives it every number,
every file name and every line of structure; it only types. Do not ask it to design, decide or compute.

## 5. Work order

After every step, the orchestrator does all of these:
1. Run `cargo test --workspace` and `npm run typecheck`. Both must pass with no warnings.
2. Bump the version with `npm version minor --no-git-tag-version`, mirror it in `server/Cargo.toml`,
   and add a CHANGELOG entry.
3. Update `docs/ARCHITECTURE.md` if files were added, then tick the box here.
4. Commit. Push only when the user asks.

- [ ] Step 0: spike (gpt-6.1-sol)
- [x] Step 1: reference tests (gemini-3.8-flash-high)
- [ ] Step 2: kernel, shapes, combine, export (gpt-6.1-sol)
- [ ] Step 3: extrude, revolve, fillet, chamfer, place (gpt-6.1-sol)
- [ ] Step 4: holes, enclosure kit, inspect (gpt-6.1-sol)
- [ ] Step 5: tool descriptions (claude-sonnet-5-5)
- [ ] Step 6: retire the FreeCAD solid tools (orchestrator)

---

### Step 0 brief: spike (send to gpt-6.1-sol)

````
You are working in the Forge repository at /home/mdhaarith/Documents/Projects/Forge (Rust, Fedora 44).
Task: find out which OpenCASCADE (OCCT) Rust binding can do what Forge needs. This is a throwaway
experiment. Do NOT edit any file outside spikes/occt/. Do NOT add anything to the workspace Cargo.toml.

1. Create a standalone crate at spikes/occt/ with its own Cargo.toml containing an empty [workspace]
   table, so it is not part of the Forge workspace.
2. Try the crates in this order, and stop at the first one where everything in step 3 works:
   a) opencascade = "0.3"   b) cadrum = "0.9"   c) occt-wasm = "5"
   You may install build tools with dnf (cmake, gcc-c++, ninja-build) if a crate needs them.
3. In src/main.rs, with that crate, do all of these and print each result:
   - make a 20×20×20 box and a cylinder (radius 5, height 20) through its middle; cut; print volume
     (expected 6429.204 ±0.01)
   - fillet the 4 vertical edges of a fresh 20 mm cube with radius 2; print volume (expected 7931.327 ±0.01)
   - chamfer the 4 vertical edges of a fresh 20 mm cube by 2; print volume (expected 7840 ±0.01)
   - revolve the rectangle (0,0)-(5,0)-(5,10)-(0,10) in the XZ plane 360° about Z; print volume
     (expected 785.398 ±0.01)
   - write the cut part to out.step, read it back, print the volume again (must match)
   - write the cut part to out.stl (binary)
   - list the faces of the cut part and print which are cylinders with their radius
   - classify points against the cut part: (2,2,10) must be inside, (10,10,10) (in the hole) outside
   - on the cylindrical face, print the outward normal at its parameter midpoint; it must point
     towards the axis (the hole's centre), not away from it
   - tessellate the cut part (deflection 0.1) and print the triangle count and the summed downward
     area of triangles whose normal z < -0.7071 (expected 321.460 ±0.5: only the bottom face,
     400 − 25π)
4. Measure: time of a clean release build (`cargo clean && time cargo build --release`), and the size
   of target/release/<binary>.
5. If a feature is not exposed by the crate, say exactly which, and whether the crate offers a way
   to call raw OCCT (for example a -sys crate with cxx bindings) to add it.

Your final message must be this table and nothing else:
| check | crate used | result | expected | pass? |
plus three lines: "clean build: <minutes>", "binary size: <MB>", "missing: <list or none>".
````

**Orchestrator check:**
- Rerun `cargo run --release` in `spikes/occt/` yourself and compare the numbers.
- **Gate:** every check passes, the clean build takes 20 minutes or less, and the binary is 150 MB or
  smaller.
- If any of these fails, show the user the table and ask before going on.
- If it passes, write the chosen crate into the "Chosen crate" line below.

Chosen crate: _(fill in after step 0)_

---

### Step 1 brief: reference tests (send to gemini-3.8-flash-high)

````
You are writing ONE test file for a Rust project. Follow these instructions exactly. Do not design
anything, do not change any number, do not edit any other file.

Repository: /home/mdhaarith/Documents/Projects/Forge
File to create: server/tests/part.rs
Files you must NOT edit: everything else. Do not edit Cargo.toml. Do not add dependencies.

The tools being tested ("part_build" and "part_inspect") do not exist yet. That is expected. Every test
must have the attribute #[ignore = "part tools not built yet"] so the file compiles and the tests are
skipped for now.

Start the file with exactly this:

mod common;
#[path = "common/tools.rs"]
mod support;

use serde_json::{json, Value};
use support::{project, run};

/// Builds a part in a fresh project and returns (project dir, the models/<name>.json contents).
async fn build(input: Value) -> (common::TempDir, Value) {
    let dir = project("part", &[] as &[(&str, &str)]);
    let name = input["name"].as_str().unwrap().to_owned();
    run("part_build", input, &dir.path).await.unwrap();
    let text = std::fs::read_to_string(dir.path.join(format!("models/{name}.json"))).unwrap();
    (dir, serde_json::from_str(&text).unwrap())
}

/// Volume of body number i in the build output.
fn volume(out: &Value, i: usize) -> f64 {
    out["bodies"][i]["volume"].as_f64().unwrap()
}

/// Asserts a is within tol of b.
fn near(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

Then write one test function for each row below. Use this exact shape for every test:

#[tokio::test]
#[ignore = "part tools not built yet"]
async fn <test name>() {
    let (_dir, out) = build(json!(<input>)).await;
    near(volume(&out, 0), <expected>, <tol>);
}

Rows: test name | input (put it inside json!(...) exactly) | expected | tol

1. box_volume | {"name":"t","steps":[{"op":"box","size":[10,10,10]}]} | 1000.0 | 0.01
2. cylinder_volume | {"name":"t","steps":[{"op":"cylinder","radius":5,"height":10}]} | 785.398 | 0.01
3. sphere_volume | {"name":"t","steps":[{"op":"sphere","radius":5}]} | 523.599 | 0.01
4. cone_volume | {"name":"t","steps":[{"op":"cone","radius1":5,"radius2":0,"height":10}]} | 261.799 | 0.01
5. extrude_rectangle | {"name":"t","steps":[{"op":"extrude","height":5,"outline":{"start":[0,0],"segments":[{"line":[20,0]},{"line":[20,10]},{"line":[0,10]},{"line":[0,0]}]}}]} | 1000.0 | 0.01
6. extrude_circle | {"name":"t","steps":[{"op":"extrude","height":10,"outline":{"circle":[0,0,5]}}]} | 785.398 | 0.01
7. revolve_full | {"name":"t","steps":[{"op":"revolve","outline":{"start":[0,0],"segments":[{"line":[5,0]},{"line":[5,10]},{"line":[0,10]},{"line":[0,0]}]}}]} | 785.398 | 0.01
8. revolve_half | {"name":"t","steps":[{"op":"revolve","angle":180,"outline":{"start":[0,0],"segments":[{"line":[5,0]},{"line":[5,10]},{"line":[0,10]},{"line":[0,0]}]}}]} | 392.699 | 0.01
9. revolve_ring | {"name":"t","steps":[{"op":"revolve","outline":{"start":[3,0],"segments":[{"line":[5,0]},{"line":[5,10]},{"line":[3,10]},{"line":[3,0]}]}}]} | 502.655 | 0.01
10. cut_cylinder_from_cube | {"name":"t","steps":[{"id":"a","op":"box","size":[20,20,20]},{"id":"b","op":"cylinder","radius":5,"height":20,"at":[10,10,0]},{"op":"cut","target":"a","tool":"b"}]} | 6429.204 | 0.01
11. union_overlapping_cubes | {"name":"t","steps":[{"id":"a","op":"box","size":[10,10,10]},{"id":"b","op":"box","size":[10,10,10],"at":[5,0,0]},{"op":"union","target":"a","tool":"b"}]} | 1500.0 | 0.01
12. intersect_overlapping_cubes | {"name":"t","steps":[{"id":"a","op":"box","size":[10,10,10]},{"id":"b","op":"box","size":[10,10,10],"at":[5,0,0]},{"op":"intersect","target":"a","tool":"b"}]} | 500.0 | 0.01
13. fillet_vertical_edges | {"name":"t","steps":[{"id":"a","op":"box","size":[20,20,20]},{"op":"fillet","target":"a","radius":2,"edges":"vertical"}]} | 7931.327 | 0.01
14. fillet_all_edges | {"name":"t","steps":[{"id":"a","op":"box","size":[20,20,20]},{"op":"fillet","target":"a","radius":2,"edges":"all"}]} | 7804.696 | 0.05
15. chamfer_vertical_edges | {"name":"t","steps":[{"id":"a","op":"box","size":[20,20,20]},{"op":"chamfer","target":"a","size":2,"edges":"vertical"}]} | 7840.0 | 0.01
16. m3_clearance_hole | {"name":"t","steps":[{"id":"a","op":"box","size":[20,20,5]},{"op":"hole","target":"a","points":[[10,10]],"size":"M3","kind":"clearance"}]} | 1954.604 | 0.01
17. m3_tap_blind_hole | {"name":"t","steps":[{"id":"a","op":"box","size":[20,20,5]},{"op":"hole","target":"a","points":[[10,10]],"size":"M3","kind":"tap","depth":4}]} | 1980.365 | 0.01
18. enclosure_volume | {"name":"t","steps":[{"op":"enclosure","size":[60,40,20]}]} | 11712.0 | 0.01
19. lid_volume | {"name":"t","steps":[{"op":"lid","size":[60,40]}]} | 8758.72 | 0.01
20. standoffs_add_volume | {"name":"t","steps":[{"id":"e","op":"enclosure","size":[60,40,20]},{"op":"standoffs","target":"e","points":[[10,10],[50,30]],"size":"M3","height":5,"base":2,"kind":"insert"}]} | 12088.991 | 0.05
21. lid_fits_with_no_overlap | {"name":"t","steps":[{"id":"e","op":"enclosure","size":[60,40,20]},{"id":"l","op":"lid","size":[60,40],"at":[0,0,20]},{"op":"intersect","target":"e","tool":"l"}]} | 0.0 | 0.001
22. lid_moved_past_gap_overlaps — this one is different: use the input
    {"name":"t","steps":[{"id":"e","op":"enclosure","size":[60,40,20]},{"id":"l","op":"lid","size":[60,40],"at":[0.25,0,20]},{"op":"intersect","target":"e","tool":"l"}]}
    and instead of near(...) write: assert!(volume(&out, 0) > 0.01);
23. lid_moved_within_gap_still_fits | same input as 22 but "at":[0.15,0,20] | 0.0 | 0.001

Then write these five extra tests exactly as given:

#[tokio::test]
#[ignore = "part tools not built yet"]
async fn place_rotates_then_moves() {
    let (_dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[10,20,5]},{"op":"place","target":"a","rotate":[0,0,90],"move":[100,0,0]}]})).await;
    let bbox = &out["bodies"][0]["bbox"];
    near(bbox[0][0].as_f64().unwrap(), 80.0, 0.001);
    near(bbox[1][0].as_f64().unwrap(), 100.0, 0.001);
    near(bbox[0][1].as_f64().unwrap(), 0.0, 0.001);
    near(bbox[1][1].as_f64().unwrap(), 10.0, 0.001);
    near(bbox[1][2].as_f64().unwrap(), 5.0, 0.001);
}

#[tokio::test]
#[ignore = "part tools not built yet"]
async fn writes_step_stl_and_json_and_step_reads_back() {
    let (dir, out) = build(json!({"name":"t","steps":[{"id":"a","op":"box","size":[20,20,20]},{"id":"b","op":"cylinder","radius":5,"height":20,"at":[10,10,0]},{"op":"cut","target":"a","tool":"b"}]})).await;
    for file in ["models/t.step", "models/t.stl", "models/t.json"] {
        assert!(dir.path.join(file).is_file(), "{file} missing");
    }
    assert_eq!(out["steps"].as_array().unwrap().len(), 3);
    let report = run("part_inspect", json!({"path":"models/t.step"}), &dir.path).await.unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    near(report["volume"].as_f64().unwrap(), 6429.204, 6.43);
    let mesh = run("part_inspect", json!({"path":"models/t.stl"}), &dir.path).await.unwrap();
    let mesh: Value = serde_json::from_str(&mesh.text).unwrap();
    near(mesh["volume"].as_f64().unwrap(), 6429.204, 130.0);
    assert_eq!(mesh["holes"], "unknown (mesh)");
}

#[tokio::test]
#[ignore = "part tools not built yet"]
async fn inspect_finds_both_m5_holes_and_mass() {
    let (dir, _out) = build(json!({"name":"bracket","steps":[{"id":"a","op":"box","size":[40,20,5]},{"op":"hole","target":"a","points":[[8,10],[32,10]],"size":"M5","kind":"clearance"}]})).await;
    let report = run("part_inspect", json!({"path":"models/bracket.step","material":"Aluminium 6061"}), &dir.path).await.unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    near(report["volume"].as_f64().unwrap(), 3762.417, 0.01);
    near(report["mass_g"].as_f64().unwrap(), 10.159, 0.001);
    let holes = report["holes"].as_array().unwrap();
    assert_eq!(holes.len(), 2);
    for hole in holes {
        assert_eq!(hole["fits"], "M5 clearance");
        assert_eq!(hole["through"], true);
        near(hole["diameter"].as_f64().unwrap(), 5.5, 0.001);
    }
    assert_eq!(report["overhang_warning"], false);
}

#[tokio::test]
#[ignore = "part tools not built yet"]
async fn inspect_warns_about_overhang() {
    let (dir, _out) = build(json!({"name":"tee","steps":[{"id":"a","op":"box","size":[10,10,10]},{"id":"b","op":"box","size":[30,10,5],"at":[-10,0,10]},{"op":"union","target":"a","tool":"b"}]})).await;
    let report = run("part_inspect", json!({"path":"models/tee.step"}), &dir.path).await.unwrap();
    let report: Value = serde_json::from_str(&report.text).unwrap();
    near(report["overhang_area"].as_f64().unwrap(), 200.0, 0.5);
    assert_eq!(report["overhang_warning"], true);
}

#[tokio::test]
#[ignore = "part tools not built yet"]
async fn errors_name_the_step() {
    let dir = project("part", &[] as &[(&str, &str)]);
    let err = run("part_build", json!({"name":"t","steps":[{"op":"box","size":[1,1,1]},{"op":"teapot"}]}), &dir.path).await.unwrap_err();
    assert!(err.to_string().contains("step 1: unknown op \"teapot\""), "{err}");
    let err = run("part_build", json!({"name":"t","steps":[{"op":"cut","target":"x","tool":"y"}]}), &dir.path).await.unwrap_err();
    assert!(err.to_string().contains("step 0: no body \"x\""), "{err}");
    let err = run("part_build", json!({"name":"t","steps":[{"id":"a","op":"box","size":[2,2,2]},{"op":"fillet","target":"a","radius":5,"edges":"all"}]}), &dir.path).await.unwrap_err();
    assert!(err.to_string().contains("step 1: fillet failed"), "{err}");
}

When the file is written, run exactly this command from the repository root:

cargo test -p forge-server --test part

It must compile and print "0 passed; 0 failed; 28 ignored" (the counts may say "28 ignored").
If it does not compile, fix ONLY server/tests/part.rs. Common fixes:
- if ToolError has no to_string, replace err.to_string() with format!("{err:?}") everywhere;
- if `project` complains about the empty list type, keep `&[] as &[(&str, &str)]` exactly.
Do not change any number, test name or JSON input to make it compile.

Your final message must be: the last 5 lines of the cargo output, and the number of test functions
in the file. Nothing else.
````

**Orchestrator check:**
- The file has 28 tests. `cargo test -p forge-server --test part` compiles and shows 28 ignored.
- Diff the numbers against the brief: none may have changed.
- No other file changed (`git status`).
- Spot-check by hand: test 20 is 11712 + 2 × π × (4² − 2²) × 5 = 12088.991.

---

### Step 2 brief: kernel, shapes, combine, export (send to gpt-6.1-sol)

````
Repository: /home/mdhaarith/Documents/Projects/Forge. Read AGENTS.md, docs/ARCHITECTURE.md and
sections 1–3 of plan.md first. Section 3 of plan.md is the contract: follow it exactly.

Goal: add the part_build tool with these ops only: box, cylinder, sphere, cone, union, cut,
intersect. Export STEP, binary STL and the JSON described in plan.md section 3.2.

OCCT binding: use the crate recorded on the "Chosen crate" line in plan.md (spikes/occt/ shows how
it was used).

Files:
- new server/src/tools/part.rs (split into server/src/tools/part/*.rs if over ~600 lines)
- server/src/tools/mod.rs: `pub mod part;` and append the tool to all_tools(), domain "3d"
- server/Cargo.toml: the OCCT crate
- scripts/setup-fedora.sh: add the build packages the crate needs (cmake, gcc-c++, ...) to PACKAGES
- docs/ARCHITECTURE.md: add part.rs to the file map

Tests: in server/tests/part.rs, remove the #[ignore] line from exactly these tests and no others:
box_volume, cylinder_volume, sphere_volume, cone_volume, cut_cylinder_from_cube,
union_overlapping_cubes, intersect_overlapping_cubes. Do not change any test body or number.
If a test seems wrong, stop and say why instead of changing it.

Rules (break none):
- Every path goes through project_path in server/src/tools/paths.rs. Never canonicalize.
- OCCT calls block: run the whole build inside tokio::task::spawn_blocking.
- Build each part on one thread. Share no OCCT objects between calls or threads.
- models/<name>.json: serde_json with preserve_order, numbers via server/src/jsfmt.rs.
- Configuration only from Settings. Never std::env::set_var.
- Never hold a std::sync::Mutex guard across .await.
- Return ToolResult { text, files } with all three written files.
- Error messages exactly as plan.md section 3.2.
- Do not touch freecad.rs, cad.rs or any other tool yet.
- Mark deliberate limits with `// shortcut: <limit>, <when to upgrade>`.

Done means all of these pass with no warnings:
  cargo test --workspace
  npm run typecheck
Your final message: the list of files changed, and the "test result" lines of cargo test --workspace.
````

**Orchestrator check:**
- Rerun both commands.
- Read the diff of `part.rs`: paths go through `project_path`, the work is in `spawn_blocking`, and
  the JSON goes through jsfmt.
- `git diff server/tests/part.rs` shows only removed `#[ignore]` lines.

---

### Step 3 brief: extrude, revolve, fillet, chamfer, place (send to gpt-6.1-sol)

````
Repository: /home/mdhaarith/Documents/Projects/Forge. Read plan.md sections 1–3 again; section 3 is
the contract. part_build already exists in server/src/tools/part.rs (step 2).

Goal: add the ops extrude, revolve, fillet, chamfer, place, with the outline and edges formats in
plan.md section 3.1. Port the edge selectors from vertical_edges() and edges_at() in
server/runners/forge_cad.py. Rotation order and origin follow place() in forge_cad.py.

Tests: in server/tests/part.rs remove #[ignore] from exactly: extrude_rectangle, extrude_circle,
revolve_full, revolve_half, revolve_ring, fillet_vertical_edges, fillet_all_edges,
chamfer_vertical_edges, place_rotates_then_moves, writes_step_stl_and_json_and_step_reads_back,
errors_name_the_step. Do not change test bodies or numbers; if one seems wrong, stop and say why.

writes_step_stl_and_json_and_step_reads_back calls part_inspect. Add a minimal part_inspect now that
reads .step/.stp/.stl and returns the JSON in plan.md section 3.3 with real size and volume. Holes,
mass and overhang come in step 4; until then:
- holes = [] for STEP and "unknown (mesh)" for STL;
- mass_g = null;
- overhang_area = 0.
Mark this with a shortcut comment.

Same rules as step 2 (project_path, spawn_blocking, one thread per part, jsfmt, Settings, no Mutex
across await, exact error messages). Done means:
  cargo test --workspace
  npm run typecheck
pass with no warnings. Final message: files changed and the "test result" lines.
````

**Orchestrator check:**
- Rerun both commands.
- Check that `errors_name_the_step` passes, and that the fillet error is reported as
  `step 1: fillet failed`, not as a crash.

---

### Step 4 brief: holes, enclosure kit, inspect (send to gpt-6.1-sol)

````
Repository: /home/mdhaarith/Documents/Projects/Forge. Read plan.md sections 1–3; section 3 is the
contract.

Goal: finish part_build and part_inspect.
1. part_build ops hole, enclosure, lid, standoffs. Port hole(), holes(), grid(), corners(),
   rounded_box(), enclosure(), lid(), standoffs() from server/runners/forge_cad.py line by line. Copy
   the tables CLEARANCE, FITS, TAP_DRILL, SOCKET_HEAD, COUNTERSUNK_HEAD, INSERT, MATERIALS with every
   number unchanged, into a Rust module server/src/tools/part/standards.rs (or a section of part.rs).
2. part_build: if "material" is given, it must be a MATERIALS key; otherwise return an error listing
   the valid names.
3. part_inspect: port holes_in(), describe_diameter(), mass_g() and overhangs() (angle 45°). Output
   exactly the JSON in plan.md section 3.3. overhang_warning = overhang_area > 1.
4. Remove the step 3 shortcut comment in part_inspect.

Tests: remove #[ignore] from every remaining test in server/tests/part.rs. Do not change test bodies or
numbers; if one seems wrong, stop and say why.

Same rules as step 2. Done means:
  cargo test --workspace
  npm run typecheck
pass with no warnings, and `grep -n ignore server/tests/part.rs` prints nothing.
Final message: files changed and the "test result" lines.
````

**Orchestrator check:**
- Rerun the commands.
- Diff the Rust tables against `forge_cad.py` line by line; every number must match.

---

### Step 5 brief: tool descriptions (send to claude-sonnet-5-5)

````
Repository: /home/mdhaarith/Documents/Projects/Forge. Edit only the description strings and JSON
schemas of the part_build and part_inspect tools in server/src/tools/part.rs (or part/mod.rs). Do not
change any logic.

The description is what Forge's in-app AI reads to decide how to use the tool. For each tool, write:
- one sentence on what it does and what files it writes;
- units (mm, degrees, Z up), and that edges are chosen by selector ("all", "vertical", {"at_z":z}),
  never by number;
- every op with its fields and defaults, copied from plan.md section 3.1;
- limits: no sketcher constraints, loft, sweep, shell, threads or drawings (use freecad_drawing for
  drawings while it exists);
- one worked example: an enclosure 80×50×25 with 2 mm walls, M3 insert standoffs at
  {"corners":[80,50,6]} on base 2, and a matching lid placed beside it at x=90.

Make the JSON schema match plan.md section 3 exactly, with "required" set correctly. Keep it valid
for server/src/tools/validate.rs (read that file to see which schema keywords are supported).
Done means `cargo test --workspace` and `npm run typecheck` pass with no warnings. Final message:
the two descriptions as they now read.
````

**Orchestrator check:** read both descriptions as the in-app AI would. Every op in section 3.1 must
appear.

---

### Step 6: retire the FreeCAD solid tools (orchestrator does this)

1. Remove `freecad_build`, `freecad_inspect` and `cad_model`, with their tests. Delete runner code that
   only they used; keep what `freecad_drawing` still needs.
2. Grep for the old tool names in the system prompt (`server/src/agent.rs`), in `web/` and in the
   docs, and update every hit.
3. Update the dependency table in `docs/ROADMAP.md` and the file map in `docs/ARCHITECTURE.md`.
4. Run the finish line:
   - Start `npm run desktop` with FreeCAD hidden from PATH.
   - Ask for "an enclosure for this board with M3 standoffs and a lid" in a test project.
   - Check that STEP, STL and the mass appear.
   - Show the user a screenshot.
5. Tick step 6 and tell the user stage 1 is done.

## 6. Pitfalls (already inside the briefs; listed here for the reviewer)

- **Paths:** go through `project_path` only; never `canonicalize`.
- **Threads:** OCCT is blocking and not thread-safe. Use `spawn_blocking`, one part per thread, and
  share no shapes between calls.
- **UI files:** JSON the UI reads keeps key order and uses jsfmt numbers.
- **Settings:** no `std::env::set_var`.
- **Mutex:** never hold a std `Mutex` across `.await`.
- **Error strings:** keep them word for word.
- **Tests:** no network, no API key, no FreeCAD.
- **STEP units:** millimetres. A re-read part must have the same bounding box.
