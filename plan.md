# Plan: Stage 2, finish Forge's own CAD (no FreeCAD needed)

Written 2026-10-13 for an AI session that has no memory of earlier chats. Earlier plans are in git:
stage 1 (Forge's own CAD kernel) at `ed752b42` and patch 0.15.1 at `49c399f2`.

**If you are the orchestrator (the session that reads this first):**
1. Read this whole file, then `AGENTS.md`, `docs/ARCHITECTURE.md` and `docs/ROADMAP.md`.
2. Find the first unticked step under "Work order".
3. Send that step's brief to the model named on it. Copy the brief exactly and add nothing.
4. Check the result with that step's "Orchestrator check".
5. Tick the box and commit. Then do the next step.

Do one step at a time. Never let two agents edit the checkout at once.

---

## 1. Where things stand

- Forge is at v0.15.1 on `main`. `part_build` and `part_inspect` (`server/src/tools/part.rs` and
  `server/src/tools/part/`) build exact solids with cadrum 0.9 (OpenCASCADE 8.0.1).
- Every kernel call holds the `kernel()` lock in `part.rs`, because OpenCASCADE crashes when two
  threads use it at once.
- Three FreeCAD tools still cover what `part_build` cannot do. They are in
  `server/src/tools/freecad.rs`, with the runner `server/runners/forge_cad.py`:
  - `freecad_build`: loft, sweep, shell, threads and sheet metal, through a FreeCAD Python script.
  - `freecad_inspect`: centre of mass, a chosen overhang angle, and interference or clearance
    between parts.
  - `freecad_drawing`: front, top and side views with hidden lines, an iso view, overall
    dimensions, a hole table, a title block, SVG, and a DXF of the top outline.
- **Goal of stage 2:** `part_build`, `part_inspect` and a new `part_drawing` do all of this, and
  then the three FreeCAD tools are deleted. `openscad_render` and `blender_build` stay.

### What the 2026-10-13 spike already showed

The spike is in `spikes/occt/src/bin/stage2.rs`. It is uncommitted and becomes step 0's starting
point. Run it with:

```sh
cd spikes/occt && CARGO_TARGET_DIR=../../target cargo run --release --bin stage2
```

| Case | Result |
|---|---|
| `Solid::loft`, ruled, 20×20 square at z=0 to 10×10 at z=10 | 2333.333 (exact frustum) |
| `Solid::loft`, ruled, circle r10 to r5 over h10 | 1832.596 (exact) |
| `Solid::loft`, smooth, squares 20/10/20 at z 0/10/20 | 3733.333 |
| `Solid::sweep`, circle r2 along a 30 mm line (`ProfileOrient::Fixed`) | 376.991 (exact) |
| `Solid::sweep`, circle r2 along a quarter arc R20 (`Up(Z)`) | 394.784 (exact) |
| `shell(-2.0, [top face])` on a 40×30×20 box | 7152 (exact; negative thickness goes inward) |
| `shell(-1.0, [top face])` on a cylinder r10 h20 | 1448.274 (exact) |
| `shell(+2.0, …)` | grows outward with rounded corners; do not use |
| `mirror(origin, normal)` | exact |
| intersection volume of two cubes overlapping by 2 mm | 200; a disjoint intersect gives 0 solids |
| `Mesh::scene(SceneOption{view, up, hidden_edges:true, shading:false})` | polylines in `edges_visible` and `edges_hidden`. A plain box front view has 4 visible and 0 hidden polylines. A box with a blind hole has hidden polylines. Coordinates are mm, with the extent equal to the part size. |
| Thread | Works. Step 0 found that the "radius 6.78" was cadrum's loose bounding box on an unmeshed helical sweep; the real radius is 3.000. The exact working pattern is in `spikes/occt/README.md` "Stage 2". The external M6 rod takes about 2.6 s and the internal thread about 1.1 s. |

## 2. What stage 2 delivers

- `part_build` gains these ops: `loft`, `sweep`, `shell`, `mirror`, `pattern`, `thread` (external
  rod), hole `kind:"thread"` (internal thread), and `sheet` (sheet metal with bends and a flat
  length).
- `part_inspect` gains `against` (other STEP files), `overhang` (degrees), `center_of_mass`, and
  `interference` (overlap volume and clearance for every pair of solids).
- A new tool, `part_drawing`, writes `drawings/<name>.svg` (views, hidden lines, dimensions, hole
  table, title block) and, with `dxf:true`, `drawings/<name>-top.dxf`.
- `freecad_build`, `freecad_inspect`, `freecad_drawing`, `freecad.rs`, `forge_cad.py` and their
  tests are deleted. The system prompt, docs and setup script are updated.
- Release v0.16.0.

## 3. The contract (every model follows this exactly)

All lengths are mm, angles are degrees, and Z is up. Every volume and length in the output JSON
goes through `jsfmt::fixed(n, 3)`. Each "Reference" below is the exact expected value in
`models/<name>.json` `bodies[i]`, unless a tolerance is stated.

### 3.1 New `part_build` ops

**`loft`**: a shape step that makes a body.
- Fields: `id`; `sections`, an array of `{"outline": <outline>, "z": number}` with at least 2
  entries and z strictly increasing; `ruled` (default false); `at`.
- Each outline is in XY at height z (same outline format as `extrude`). The body is then moved by
  `at`.
- Errors:
  - `step <i>: loft needs at least 2 sections with increasing z`
  - `step <i>: loft failed: <err>`
- Reference:
  - ruled, outline squares `[-10,-10]..[10,10]` at z=0 and `[-5,-5]..[5,5]` at z=10: volume
    2333.333, bbox [[-10,-10,0],[10,10,10]]
  - ruled, circles `[0,0,10]` at z=0 and `[0,0,5]` at z=10: volume 1832.596
  - smooth, squares of half-size 10, 5 and 10 at z 0, 10 and 20: volume 3733.333

**`sweep`**: a shape step.
- Fields: `id`; `outline`, the profile drawn in its own XY plane around its origin; `path`,
  `{"start":[x,y,z],"segments":[{"line":[x,y,z]} | {"arc":[x,y,z],"through":[x,y,z]}, …]}`.
- The profile is placed with its origin at `path.start` and its +Z along the path's start tangent
  (`align_z`, then `translate`).
- Consecutive segments must meet with tangents within 1°. Otherwise return the error below.
- Use `ProfileOrient::Fixed` for a single straight line, and `Up` with a direction that is not
  parallel to the path otherwise.
- Errors:
  - `step <i>: path segments must meet tangentially; join corners with an arc`
  - `step <i>: sweep failed: <err>`
- Reference (profile `{"circle":[0,0,2]}`):
  - path from [0,0,0], line to [30,0,0]: volume 376.991
  - path from [20,0,0], arc to [0,20,0] through [14.142136,14.142136,0]: volume 394.784
  - path from [0,0,0]: line to [10,0,0], then arc to [20,10,0] through [17.071068,2.928932,0],
    then line to [20,20,0]: volume 448.720

**`shell`**: an operation on `target`.
- Fields: `thickness`, the wall measured inward; `open`, an array of sides from `"top"`,
  `"bottom"`, `"front"` (−Y), `"back"` (+Y), `"left"` (−X) and `"right"` (+X). Default `["top"]`.
- A side selects the planar faces whose outward normal points that way and that lie at the body's
  bounding-box extreme on that side.
- Calls `solid.shell(-thickness, faces)`.
- Errors:
  - `step <i>: shell found no <side> face`
  - `step <i>: shell failed: <err>`
- Reference:
  - box 40×30×20, thickness 2, open top: 7152
  - cylinder r10 h20, thickness 1: 1448.274

**`mirror`**: an operation on `target`.
- Fields: `plane`, one of `"x"`, `"y"` or `"z"` (the plane's normal axis); `origin` (default
  [0,0,0]); `keep` (default true).
- With `keep:true` the result is the union of the original and the mirror image. With
  `keep:false` the mirror image replaces the original.
- Reference:
  - box `[5,0,0]+[10,10,10]`, plane x, keep: volume 2000, 2 solids, bbox [[-15,0,0],[15,10,10]]
  - box `[0,0,0]+[10,10,10]`, plane x: volume 2000, 1 solid
  - `keep:false`: volume 1000, bbox [[-15,0,0],[-5,10,10]]

**`pattern`**: an operation on `target`. The result is the union of all copies, and the count
includes the original.
- Fields: exactly one of:
  - `linear: {"step":[dx,dy,dz],"count":n}`
  - `polar: {"count":n,"angle":360,"center":[x,y]}`: about a Z axis through `center` (default
    [0,0]). With `angle` 360 the copies are 360/n apart; otherwise they are angle/(n−1) apart.
- Errors:
  - `step <i>: pattern needs linear or polar`
  - `step <i>: count must be an integer from 2 to 100`
- Reference:
  - box 10³ at origin, `linear {step:[20,0,0],count:3}`: volume 3000, 3 solids, bbox
    [[0,0,0],[50,10,10]]
  - cylinder r2 h10 at [10,0,0], `polar {count:6}`: volume 753.982, 6 solids, bbox
    [[-12,-10.66,0],[12,10.66,10]]
- Usage tip for the description: pattern a cutter body, then `cut` with it.

**`thread`**: a shape step that makes an external threaded rod.
- Fields: `id`; `size` (`"M2"`, `"M2.5"`, `"M3"`, `"M4"`, `"M5"`, `"M6"`, `"M8"`, `"M10"` or
  `"M12"`); `length`; `at`. The axis is +Z from `at`.
- Use the ISO metric coarse pitch: M2 0.4, M2.5 0.45, M3 0.5, M4 0.7, M5 0.8, M6 1, M8 1.25,
  M10 1.5, M12 1.75. Add it as a `PITCH` table in `standards.rs`.
- Profile: ISO basic 60° profile. The major diameter is d, with the crest flat on d. The rod is
  clipped exactly to z ∈ [0, length].
- Error: `step <i>: thread size must be one of M2, M2.5, M3, M4, M5, M6, M8, M10, M12`
- Reference, M6 length 10 at the origin:
  - bbox [[-3,-3,0],[3,3,10]] within 0.01
  - 1 solid
  - volume within 0.5 % of 229.572 (the ISO basic profile integrated exactly): 228.42 to 230.72
  - `thread` meshes its result at 0.005 mm before returning it, because cadrum's
    `bounding_box` is loose (radius 6.78 for M6) on unmeshed helical faces. Do not clamp or
    round the box instead.

**hole `kind:"thread"`**: an internal thread.
- Same fields and placement as other hole kinds (down from the target's top, `depth` omitted means
  through). Drills the tap-drill hole, then cuts the ISO internal thread.
- Reference: box 20×20×10, an M6 thread at [10,10], through. The removed volume
  (4000 − the body's volume) is within 0.5 % of 231.333 (tap drill plus the ISO flanks):
  230.18 to 232.49.
- `part_inspect` does not need to recognise threaded holes:
  `shortcut: threaded holes are not reported by part_inspect, add helical-face detection when users ask`.

**`sheet`**: a shape step for sheet metal.
- Fields: `id`; `thickness`; `width` (along +Y); `flanges` (flat lengths, at least 1); `bends`
  (`[{"angle":deg,"radius":inner}]`, exactly flanges−1 entries; radius defaults to thickness);
  `k_factor` (default 0.44); `at`.
- Geometry: the first flange runs +X from x=0 with z ∈ [0, thickness]. A positive angle bends
  towards +Z: the inside of the bend is the upper face, centred at (end of flange,
  thickness + radius). Each later flange continues from the bend. The XZ section is extruded +Y by
  `width`, then moved by `at`.
- Flat length = Σflanges + Σ (angle in radians)·(radius + k_factor·thickness).
- Output: the sheet body's entry in `models/<name>.json` gets `"flat":[flat_length, width]` after
  `"solids"`. The entry keeps it through later ops on that body.
- Error: `step <i>: sheet needs one more flange than bends`
- Reference (thickness 2, width 10, radius 2, k 0.44):
  - flanges [30,20], bends [{angle:90}]: volume 1094.248, bbox [[0,0,0],[34,10,24]],
    flat [54.524,10]
  - flanges [20,30,20], bends [{angle:90},{angle:90}]: volume 1588.496, bbox
    [[0,0,0],[24,10,38]], flat [79.048,10]
- Limit: `shortcut: no flat-pattern DXF or holes in the flat, add when a user needs laser files from sheet parts`.

**Schema.** Add every new field to the `part_build` step schema with types, following the rules of
plan 0.15.1 §3.2. These rules are enforced by `every_tool_schema_is_portable`, so keep
`additionalProperties:false`:
- `op` enum: add loft, sweep, shell, mirror, pattern, thread, sheet
- `kind`: add `thread`
- new fields: `sections`, `ruled`, `path`, `open`, `plane`, `origin`, `keep`, `linear`, `polar`,
  `length`, `flanges`, `bends`, `k_factor`, `width`

Update the `part_build` description: list each new op in the same style, remove "loft, sweep,
shell, threads or drawings" from Limits, and point drawings at `part_drawing`.

### 3.2 `part_inspect` additions

- **New input:**
  - `against`: an array of project-relative `.step`/`.stp` files
  - `overhang`: degrees from vertical, default 45
- **New output keys, in this order:** file, size, volume, **center_of_mass**, material, mass_g,
  holes, overhang_area, overhang_warning, **interference**.
- `center_of_mass` is `[x,y,z]`:
  - STEP: volume-weighted `Solid::center`
  - STL: from signed tetrahedra
  - fixed to 3 decimal places
- `interference` has one entry per pair of solids. The solids are every solid in `path`, then
  every solid in each `against` file, labelled `<file>#<n>` with n counting from 1 per file. Each
  entry is:
  ```json
  {"a":"models/case.step#1","b":"models/lid.step#1","overlap_volume":v,"clearance":c}
  ```
  - `overlap_volume` is the volume of the intersection.
  - `clearance` is the smallest distance between the two surfaces, and 0 when they touch or
    overlap. Compute it from 0.01 mm tessellations as the minimum of vertex-to-triangle distance
    both ways and triangle-edge to triangle-edge (segment-segment) distance, so crossed edges
    count. Skip triangle pairs whose bounding boxes are farther apart than the best distance
    so far.
    Add: `shortcut: clearance comes from tessellations (exact for flat faces, within 0.01 mm on curves), use BRepExtrema when cadrum exposes it`.
  - With fewer than two solids, `interference` is `[]`. For an STL `path` it is always `[]`.
- **Errors:**
  - `against files must be .step or .stp`
  - `overhang must be between 0 and 90`
- **Reference:**
  - box 2×3×4 at the origin: center_of_mass [1,1.5,2]
  - two 10 mm cubes, [0,0,0] and [8,0,0]: overlap_volume 200, clearance 0
  - cubes at [0,0,0] and [13,0,0]: overlap_volume 0, clearance 3
  - cubes at [0,0,0] and [13,14,0]: overlap_volume 0, clearance 5
  - the existing STL test report gains `"center_of_mass":[0.25,0.25,0.25]` (tetrahedron centroid)
    and `"interference":[]`
- Update the description: remove the "use freecad_drawing" sentence and describe the new fields.

### 3.3 New tool `part_drawing` (`server/src/tools/part/drawing.rs`, domain "3d")

- **Input:**
  - `path` (required): a `.step` or `.stp` file
  - `name`: default the file stem; lowercase letters, digits, `-` and `_`
  - `title`: default name
  - `material`: a MATERIALS key, for mass
  - `projection`: `"first"` (default) or `"third"`
  - `sheet`: `"auto"` (default), `"A4"`, `"A3"` or `"A2"`
  - `dxf`: default false
- **Views:**
  - front looks along +Y (the camera at −Y), up Z
  - top looks along −Z, up +Y
  - right looks along −X, up Z
  - iso uses `view (1,-1,1)`, up Z
  - Each view comes from `Mesh::scene` with `hidden_edges:true, shading:false` and a 0.05 mm
    tessellation.
  - Visible polylines are `<path class="visible">` with stroke width 0.5.
  - Hidden polylines are `<path class="hidden">` with `stroke-dasharray="2 1"` and width 0.25.
  - The iso view has no hidden lines and is not to scale.
- **Layout:** the sheet is landscape, in mm, with the SVG `viewBox` equal to the sheet size.
  - Sizes: A4 297×210, A3 420×297, A2 594×420. Use 10 mm margins and a 40 mm title block along
    the bottom.
  - The drawing area is (W−20) × (H−20−40).
  - Orthographic views are placed in an L with 20 mm gaps. The space needed is
    ((X+Y)·s + 60) × ((Y+Z)·s + 60), where X, Y and Z are the part's size.
  - First angle: front at top-left, top below it, right view to the left of front. Third angle:
    top above front, right view to the right of front. Mirror the L to fit. The iso view goes in
    the free corner.
- **Scale:** the largest of 5, 2, 1, 1/2, 1/5, 1/10, 1/20 and 1/50 that fits. Written as `"5:1"`,
  `"1:1"`, `"1:2"` and so on.
  - `sheet:"auto"`: the first of A4, A3 and A2 where scale 1:1 fits. If none fits, use A2 at the
    largest reducing scale that fits.
  - If nothing fits: `part does not fit on A2 at 1:50`.
- **Annotations:**
  - Overall dimensions on the views as text: X and Y on the top view, Z on the front view. Numbers
    are formatted by `jsfmt` (so `40`, not `40.0`).
  - Each hole from `holes::holes_in` with axis Z gets a tag `H1`, `H2`, … at its centre on the top
    view.
  - A hole table has columns Tag, Size (the hole's `fits` text or `⌀<diameter>`),
    Depth (`THRU` or the depth), X and Y (from the part's minimum corner).
  - The title block shows the title, material (or `—`), mass in g (or `—`), scale, sheet,
    projection (`first angle`/`third angle`) and `ISO 2768-m`.
- **DXF** (`dxf:true`): `drawings/<name>-top.dxf`, ASCII R12.
  - Contents: the top view's visible polylines at 1:1, with the part's minimum corner at (0,0), as
    `LINE` entities on layer `OUTLINE`.
  - Add: `shortcut: arcs are written as short lines, emit ARC entities when cadrum exposes curve types`.
- **Output:**
  - Text: one JSON object, `{"svg":"drawings/<name>.svg","dxf":<path or null>,"sheet":"A4","scale":"1:1","views":["front","top","right","iso"],"holes":<count>}`.
  - `files` lists the written files.
  - `shortcut: no PDF output, print the SVG from a browser; add PDF when a renderer is bundled`.
- **Errors:**
  - `path must be a .step or .stp file`
  - `name may only use lowercase letters, digits, - and _`
  - `part does not fit on A2 at 1:50`
- **Reference:**
  - A 40×30×20 box: A4, `"1:1"`. The front view has 0 hidden paths. The SVG contains the texts
    `40`, `30` and `20`.
  - The same box with an M6 clearance hole at [20,15], depth 10:
    - the front view has more than 0 hidden paths
    - the hole table has one row `H1`, with Size `M6 clearance` and Depth `10`
  - A 400×300×200 box: A2 at `"1:2"`.
  - A 10×10×5 box: A4 at `"5:1"`.
  - With `dxf:true` on the 40×30×20 box, the DXF has `ENTITIES`, at least 4 `LINE` entities and
    layer `OUTLINE`, and the coordinates span x 0..40 and y 0..30.
- Tag each view's group as `<g id="view-front">`, `<g id="view-top">`, `<g id="view-right">` and
  `<g id="view-iso">` so that tests can count paths per view.
- Register the tool in `part.rs` `tools()` and run it in `spawn_blocking` under `kernel()`, like
  `part_inspect`.

### 3.4 Retiring FreeCAD

Delete all of these, and their mentions in `tools/mod.rs`, `agent.rs` (prompt guidance), tests,
`README.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md` and `scripts/setup-fedora.sh`:
- `freecad_build`, `freecad_inspect` and `freecad_drawing`
- `server/src/tools/freecad.rs`
- `server/runners/forge_cad.py` and any other FreeCAD-only runner
- `server/tests/freecad.rs`
- the `FREECAD` app spec and `FORGE_FREECAD`

Keep `FORGE_NO_FLATPAK`, which Blender still uses. The prompt's 3D guidance becomes:
- `part_build` for every exact part
- `part_inspect` for checking, fit between parts (`against`) and print overhangs
- `part_drawing` for drawings
- `blender_build` for visual models

## 4. Models: who does what

| Role | Model | `delegate_task` target |
|---|---|---|
| Spike and code | GPT-6.1-Sol | codex `gpt-6.1-sol`, options `{"reasoningEffort":"high","serviceTier":"default"}` |
| Tests | Gemini 3.8 Flash | antigravity `gemini-3.8-flash-high` |
| Review, check, version, commit | Orchestrator (Claude) | — |

Use a distinct `clientRequestId` per round. If a step fails review, send a new round with the brief,
the findings and the agent's previous report.

## 5. Work order

- [x] Step 0: spike (GPT)
- [x] Step 1: reference tests (Gemini)
- [x] Step 2: loft, sweep, shell, mirror, pattern (GPT)
- [x] Step 3: threads and sheet metal (GPT)
- [x] Step 4: part_inspect interference, centre of mass, overhang (GPT)
- [x] Step 5: part_drawing (GPT)
- [x] Step 6: retire FreeCAD (GPT)
- [x] Step 7: release 0.16.0 and finish line (orchestrator)

### Step 0 brief: spike (send to gpt-6.1-sol)

> You work on Forge, a Rust CAD app built on cadrum 0.9 (OpenCASCADE). Read `AGENTS.md`, then
> `plan.md` sections 1 and 3. Work only in `spikes/occt/`. Its spike binary
> `src/bin/stage2.rs` already exists; run it with
> `cd spikes/occt && CARGO_TARGET_DIR=../../target cargo run --release --bin stage2`. The cadrum
> source is in `~/.cargo/registry/src/*/cadrum-0.9.0/`.
>
> Extend the spike until it proves each item below with printed numbers:
> 1. External ISO M6×1 thread rod, length 10: bbox [[-3,-3,0],[3,3,10]] within 0.01, 1 solid, and
>    volume within 3 % of 224.84. Today the swept profile lands at radius 6.78. Find out why
>    (profile placement or orientation in `make_pipe_shell`) and fix it using only cadrum's public
>    API. Report the time taken.
> 2. Internal M6 thread through a 20×20×10 box: removed volume within 3 % of 224.84.
> 3. Sheet metal L bracket per plan.md 3.1 `sheet`: volume 1094.248, bbox [[0,0,0],[34,10,24]].
> 4. The sweep path with a line, an arc and a line from plan.md 3.1 `sweep`: volume 448.720.
> 5. Polar pattern of 6 cylinders r2 h10 at radius 10: volume 753.982.
> 6. Mesh clearance between cubes at [0,0,0] and [13,14,0]: 5.000.
> 7. An SVG with front, top, right and iso views of a 40×30×20 box with a blind r4 hole, depth 10,
>    from the top, using `Mesh::scene`, hidden lines dashed, written to `spikes/occt/stage2.svg`.
>
> Save the full output to `spikes/occt/results-stage2.log`. Add a short "Stage 2" section to
> `spikes/occt/README.md`: what works, what needed a workaround and the exact workaround code
> pattern. If any reference number in plan.md is wrong, say which and show the arithmetic. Do not
> touch `server/`. Report the log and the workarounds.

**Orchestrator check:**
- Read the log.
- Open `stage2.svg`.
- Confirm that every number matches or that the plan is corrected with arithmetic.
- Copy each workaround into the step 2 or step 3 brief as a pitfall if needed.

### Step 1 brief: reference tests (send to gemini-3.8-flash-high)

> You write tests for Forge, a Rust server in `server/`. Read `AGENTS.md`, `plan.md` section 3 and
> `server/tests/part.rs` (the style to follow: build through `part_build`, read
> `models/<name>.json`). Create `server/tests/part_advanced.rs`. Do not edit any other file.
>
> Write one test per Reference bullet in plan.md 3.1, 3.2 and 3.3, plus one test per error string
> listed there. Compare exact JSON numbers, except where the plan gives a tolerance or a range.
> Mark every test `#[ignore = "stage 2 not built yet"]`. Group them by section, with test names
> that start with `loft_`, `sweep_`, `shell_`, `mirror_`, `pattern_`, `thread_`, `sheet_`,
> `inspect_` or `drawing_`, so each build step can un-ignore its own group.
>
> For drawings, parse the SVG text: count `<path class="hidden"` inside `<g id="view-front">`, and
> find the `<text>` contents. Tests must not need the network or FreeCAD. Run
> `cargo test --workspace`: everything else must pass and your tests must be ignored. Report the
> test names and the output tail.

**Orchestrator check:**
- The diff touches only `server/tests/part_advanced.rs`.
- Every Reference bullet and every error string has a test.
- The numbers match the plan.
- The suite passes with the new tests ignored.

### Step 2 brief: loft, sweep, shell, mirror, pattern (send to gpt-6.1-sol)

> You code Forge, a Rust server in `server/`. Read `AGENTS.md`, all of `plan.md`, and
> `spikes/occt/README.md` "Stage 2". Implement the `part_build` ops `loft`, `sweep`, `shell`,
> `mirror` and `pattern` exactly as in plan.md 3.1. Put the code in `server/src/tools/part/ops.rs`,
> or in a new `server/src/tools/part/shapes.rs` if ops.rs would pass about 600 lines. Add their
> schema fields and description text. Un-ignore the tests in `server/tests/part_advanced.rs` whose
> names start with `loft_`, `sweep_`, `shell_`, `mirror_` or `pattern_`, and make them pass
> without changing what they assert. Keep every existing error message word for word. No new
> crates. Done means `cargo test --workspace` passes and `cargo clippy --workspace` shows no
> warnings. Report the diff summary and the test tail.

**Orchestrator check:**
1. Read the diff.
2. Rerun the tests 3 times, since OpenCASCADE crashes show up only now and then.
3. Run `every_tool_schema_is_portable`.

### Step 3 brief: threads and sheet metal (send to gpt-6.1-sol)

> You code Forge, a Rust server in `server/`. Read `AGENTS.md`, all of `plan.md`, and
> `spikes/occt/README.md` "Stage 2" (it contains the working thread workaround). Implement the
> `thread` op, hole `kind:"thread"` and the `sheet` op exactly as in plan.md 3.1, including the
> `PITCH` table in `standards.rs`, the `flat` entry in the body JSON, and their schema and
> description. Un-ignore the `thread_` and `sheet_` tests in `server/tests/part_advanced.rs` and
> make them pass without changing what they assert. Existing error messages stay word for word. No
> new crates. Done means `cargo test --workspace` passes and `cargo clippy --workspace` shows no
> warnings. Report the diff summary, the test tail and how long the thread tests take.

**Orchestrator check:**
- Read the diff and rerun the tests 3 times.
- Make sure the thread tests stay under 10 s in total.
- Build an M8 bolt in the offline UI and screenshot it.

### Step 4 brief: part_inspect additions (send to gpt-6.1-sol)

> You code Forge, a Rust server in `server/`. Read `AGENTS.md` and all of `plan.md`. Implement
> plan.md 3.2 in `server/src/tools/part/inspect.rs`: `against`, `overhang`, `center_of_mass` and
> `interference`, with the exact key order, labels and errors, and the clearance shortcut comment.
> Update the existing inspect tests that compare the whole report so they include the new keys.
> Un-ignore the `inspect_` tests in `server/tests/part_advanced.rs` and make them pass. Update the
> description and schema. No new crates. Done means `cargo test --workspace` passes and
> `cargo clippy --workspace` shows no warnings. Report the diff summary and the test tail.

**Orchestrator check:**
- Read the diff and rerun the tests 3 times.
- Check the `design_assemble` description still makes sense next to `against`.

### Step 5 brief: part_drawing (send to gpt-6.1-sol)

> You code Forge, a Rust server in `server/`. Read `AGENTS.md`, all of `plan.md`, and
> `spikes/occt/README.md` "Stage 2" (including `spikes/occt/stage2.svg`). Create
> `server/src/tools/part/drawing.rs` with the `part_drawing` tool exactly as in plan.md 3.3:
> input, views, layout, scale, annotations, DXF, output JSON, errors and shortcut comments. Reuse
> `holes::holes_in`, `standards` (materials) and `jsfmt`. Register it next to `part_inspect`.
> Write a description in the same style as `part_inspect`'s, with one example input. Un-ignore the
> `drawing_` tests in `server/tests/part_advanced.rs` and make them pass. No new crates. Done means
> `cargo test --workspace` passes and `cargo clippy --workspace` shows no warnings. Report the
> diff summary and the test tail.

**Orchestrator check:**
1. Read the diff.
2. Rerun the tests.
3. Render the 40×30×20 box with a hole and the 400×300×200 box, open both SVGs in a headless
   browser, and screenshot them. Check that the views do not overlap, the hidden lines are dashed,
   and the title block and hole table are readable.
4. Send the screenshots to the user.

### Step 6 brief: retire FreeCAD (send to gpt-6.1-sol)

> You code Forge. Read `AGENTS.md` and `plan.md` 3.4. Delete the FreeCAD tools and everything
> listed in 3.4, and update every mention (grep the repo for `freecad`, `FreeCAD`, `FORGE_FREECAD`
> and `forge_cad`, case-insensitive). Keep Blender and `FORGE_NO_FLATPAK` working. Rewrite the
> prompt's 3D guidance as 3.4 says. Update `docs/ARCHITECTURE.md` (file map) and
> `docs/ROADMAP.md` (stage 2 done). Done means `cargo test --workspace` passes,
> `cargo clippy --workspace` shows no warnings, `npm run typecheck` passes, and
> `grep -ri freecad` finds only CHANGELOG.md history and `plan.md`. Report the diff summary.

**Orchestrator check:**
- Read the diff and rerun all checks.
- Confirm that no test still skips for missing FreeCAD.

### Step 7: release 0.16.0 and finish line (orchestrator)

1. Run `npm version minor --no-git-tag-version` and mirror the version in `server/Cargo.toml`.
2. Add a CHANGELOG entry and commit.
3. Finish line: start Forge with Claude, using `FORGE_NO_FLATPAK=1` in a fresh `FORGE_HOME`. Ask:
   "Design a 60 × 40 × 25 mm enclosure with 2 mm walls made with shell, M3 threaded holes in the
   corners of a lid, check the lid fits, and make a drawing."
4. It passes when Claude uses only `part_build`, `part_inspect` and `part_drawing`, the drawing
   opens, and the clearance between the lid and the box is reported.
5. Screenshot the result for the user.

## 6. Pitfalls (already inside the briefs; listed here for the reviewer)

- Every kernel call, including dropping shapes, holds `kernel()`. Tool entry points run in
  `spawn_blocking`.
- Never `canonicalize`. Every written path goes through `project_path` (`models/`, `drawings/`).
- JSON numbers go through `jsfmt::fixed`. Keep the key order exactly as in the plan.
- Booleans on threads are slow (about 1 s each). For several threaded holes, build one thread
  cutter, copy it to each point, and cut once.
- Use the thread pattern from `spikes/occt/README.md` exactly: ISO flank widths, core and helix
  both extended one pitch at each end, fuse, then clip. A core shorter than the helix gives a
  ridge-only union.
- In SVG, join the scene's NaN-separated fragments into polylines before writing them, or the
  dashes restart on every fragment and hidden circles look solid. Negate Y for SVG.
- `shell` with a positive thickness rounds the outer corners. Always pass `-thickness`.
- `Mesh::scene` panics when `up` is parallel to `view`. The top view uses up +Y.
- Never call `std::env::set_var` in tests. Never hold a `std::sync::Mutex` guard across `.await`.
