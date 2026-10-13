# Plan: Stage 3, Forge's own PCB engine (no Node, no KiCad needed)

Written 2026-10-13 for an AI session that has no memory of earlier chats. Earlier plans are in git:
stage 1 at `ed752b42`, stage 2 (Forge's own CAD, FreeCAD retired) at `e11ad80b`.

**If you are the orchestrator (the session that reads this first):**
1. Read this whole file, then `AGENTS.md`, `docs/ARCHITECTURE.md` and `docs/ROADMAP.md`.
2. Find the first unticked step under "Work order".
3. Send that step's brief to the model named on it. Copy the brief exactly and add nothing.
4. Check the result with that step's "Orchestrator check".
5. Tick the box and commit. Then do the next step.

Do one step at a time. Never let two agents edit the checkout at once.

---

## 1. Where things stand

- Forge is at v0.16.0 on `main`.
- `pcb_build` (`server/src/tools/board.rs`) runs tscircuit in a sandboxed Node child
  (`engines/pcb/runner.mjs`, bundled by `npm run pcb-engine`). The model writes `pcb/<name>.tsx`.
- `pcb_drc` and `pcb_export_fab` (`server/src/tools/pcb.rs`) shell out to `kicad-cli` on a
  `.kicad_pcb` file. No Forge tool writes `.kicad_pcb`, so in practice they only serve imported boards.
- Consumers of `pcb_build` output, which must keep working unchanged:
  - `web/main.ts` and `web/design.ts` read `pcb/<name>.board.json` (`BoardBoxes`), `pcb/<name>.glb`,
    `pcb/<name>-pcb.svg` and `pcb/<name>-schematic.svg`.
  - `design_assemble` (`server/src/tools/design.rs`) reads `.glb` + `.board.json`.
  - `rf_antenna_sim` (`server/runners/openems_antenna.py`) reads `pcb/<name>.circuit.json`: the
    element types `pcb_board`, `pcb_trace` (with `is_antenna_trace`, `pcb_component_id`),
    `pcb_smtpad`, `pcb_plated_hole`, `pcb_via`, `pcb_copper_pour` (`brep_shape.outer_ring.vertices`),
    `pcb_component`, `source_component`.
- KiCad 10.0.0 is installed on the dev machine (`/usr/share/kicad/footprints`). Use it only to
  check numbers by hand; Forge and its tests must not need it.

## 2. What stage 3 delivers

Ported from KiCad (feature list: `docs/reference/app-features.md`), grouped into existing tools:

| Tool | Change |
|---|---|
| `pcb_build` | Native Rust engine in `server/src/tools/pcb/`. Input `pcb/<name>.pcb.json` (section 3). Does netlist + connectivity check (light ERC), IPC-7351 footprint generator, auto-placement, autorouter (2 or 4 layers, vias), copper pours with thermal reliefs, keepouts, printed antennas, net classes, teardrops; writes the same outputs as today plus `pcb/<name>.step`. |
| `pcb_drc` | Native DRC on the built board: clearance, track width, annular ring, drill size, copper-to-edge, unrouted nets, overlapping courtyards, per net class. Input becomes `{"board":"<name>"}`. Writes `fab/<name>/<name>-drc.txt`. |
| `pcb_export_fab` | Deleted: `pcb_build` already writes every fab file natively. |
| `rf_calc` | New ops from KiCad's PCB Calculator: `track_width` (IPC-2221, as KiCad's calculator), `via` (current and resistance), `fusing` (Onderdonk), `spacing` (IPC-2221B table), `e_series` (nearest E12/E24/E96 value and best divider pair). |

Removed at the end: `engines/pcb/`, `run_engine`, `node_program` setting if unused, the `.tsx`
format, `kicad-cli` from every tool. Release v0.17.0.

Not in stage 3 (later stages): SPICE (`circuit_simulate`), diff-pair length/skew tuning, Gerber
viewer, KiCad file export, ODB++/IPC-2581. Skipped for good: interactive editor actions, foreign
importers, legacy/niche formats, CvPcb, drawing-sheet editor, plugins.

## 3. The contract (every model follows this exactly)

### 3.1 Input: `pcb/<name>.pcb.json`

The model writes it with `write_file`, then calls `pcb_build {"source":"pcb/<name>.pcb.json"}`.
Units mm, degrees; origin at the board centre, +y up. Unknown keys are an error.

```json
{
  "board": {"width": 30, "height": 20, "layers": 2, "thickness": 1.6},
  "rules": {"trace": 0.2, "clearance": 0.2, "via": [0.6, 0.3], "edge": 0.3},
  "netclasses": {"power": {"nets": ["VCC", "GND"], "trace": 0.4}},
  "parts": [
    {"ref": "R1", "kind": "resistor", "value": "1k", "footprint": "0402", "at": [-6, 0]},
    {"ref": "LED1", "kind": "led", "value": "red", "footprint": "0603", "at": [6, 0], "rotation": 90},
    {"ref": "J1", "kind": "header", "footprint": "pinheader_1x2", "at": [0, -6]},
    {"ref": "U1", "kind": "chip", "footprint": "soic8", "pins": {"1": "VCC", "8": "GND"}, "side": "bottom", "lcsc": "C123"}
  ],
  "nets": {"IN": ["J1.1", "R1.1"], "A": ["R1.2", "LED1.A"], "GND": ["LED1.K", "J1.2"]},
  "holes": [{"at": [12, 7], "d": 3.2}],
  "pours": [{"net": "GND", "layer": "bottom"}],
  "keepouts": [{"rect": [-3, 6, 24, 6], "layers": ["bottom"]}],
  "traces": [{"net": "RF", "width": 1.1}]
}
```

- `board.outline` (optional) is `[[x,y],…]`, replacing width/height for the shape; `layers` is 2 or 4.
- `rules` defaults: trace 0.2, clearance 0.2, via [0.6, 0.3], edge 0.3. Net classes override `trace`.
- Part `kind`: `resistor capacitor inductor led diode crystal button switch chip header antenna`.
  `at` is optional: parts without it are auto-placed. `side` is `top` (default) or `bottom`.
- Pin names: numbers `"1".."n"` always work. Aliases: led/diode `A`,`K`; chip labels from `pins`.
  An unknown pin is an error: `R1.3: R1 (0402) has pins 1, 2`.
- `kind:"antenna"` takes `shape` (the 5 shapes in today's `pcb_build` description) and no footprint;
  pin 1 is the feed, pin 2 the ground short for inverted-F. Port the arm geometry from tscircuit's
  generator (MIT, under `engines/pcb/node_modules`) and add it to the third-party notices.
- `keepouts[].rect` is `[cx, cy, w, h]`.

### 3.2 Footprints (generated, IPC-7351B nominal density)

`0402 0603 0805 1206`, `sot23 sot23_5 sot223`, `soicN` (N 8..16), `tssopN` (8..28), `qfnN`
(16..48, 0.5 pitch, exposed pad), `dipN` (4..40), `bgaN` (square, 0.8 pitch), `pinheader_1xN` and
`pinheader_2xN` (2.54 pitch), `crystal_hc49`, `crystal_3225`, `button_6x6`, `sod123`. Each has pads,
courtyard, silkscreen outline, pin-1 mark, and a 3D body box (height per package).

**Reference** (step 0, `spikes/pcb/README.md`; tests assert the calculated values exactly):

| Footprint | Pad (mm) | Opposite-pad centres (mm) | KiCad pad / centres |
|---|---|---|---|
| `0402` | 0.59 × 0.64 | 0.97 | 0.54 × 0.64 / 1.02 |
| `0603` | 0.875 × 0.95 | 1.575 | 0.80 × 0.95 / 1.65 |
| `soic8` | 1.95 × 0.60 | 4.95 | same |
| `qfn32` | 0.875 × 0.25, EP 3.1 × 3.1 | 4.875 | same (`QFN-32-1EP_5x5mm_P0.5mm_EP3.1x3.1mm`) |

Every generated footprint stays within 0.1 mm of KiCad's matching footprint. Use the spike's
formulas (README "Footprints"), including `CS = sqrt(CL² + 2*CT²)` for IC leads. Pads are
rectangles: `shortcut: rectangular SMD pads, upgrade to rounded rectangles if a fab or paste
stencil needs them`.

### 3.3 Outputs (names unchanged from today)

- `pcb/<name>-pcb.svg`, `pcb/<name>-schematic.svg`, `pcb/<name>.glb`, `pcb/<name>.step`,
  `pcb/<name>.board.json` (`BoardBoxes`, same shape as `web/design.ts`), `pcb/<name>.circuit.json`
  (only the element types listed in section 1, same field names, so `rf_antenna_sim` works).
- `fab/<name>/`: `<name>-F_Cu.gbr <name>-B_Cu.gbr` (+ `In1_Cu In2_Cu` on 4 layers),
  `-F_Mask -B_Mask -F_Paste -B_Paste -F_Silkscreen -B_Silkscreen -Edge_Cuts` `.gbr` (Gerber X2,
  RS-274X, mm, 4.6), `<name>-PTH.drl` and `<name>-NPTH.drl` (Excellon, mm),
  `<name>-pnp.csv` (`Designator,Mid X,Mid Y,Layer,Rotation`), `<name>-bom.csv`
  (`Comment,Designator,Footprint,LCSC Part #`).
- JSON written with `preserve_order` and numbers through `server/src/jsfmt.rs`.
- Report text keeps today's lines: `Built pcb/<file>: board W × H mm, T mm thick, L copper layers.`,
  `N part(s): …`, `… routed as N trace(s) with N via(s); N hole(s).`, `Copper pours: GND on bottom…`,
  `No errors: every connection is routed.` or `N ERROR(S), fix them and rebuild:`.

### 3.4 Engine rules

- Autorouter: port `spikes/pcb/src/route.rs`: grid A* (grid = max(clearance/2, 0.05)), 4
  neighbours + layer change, step cost 10, via cost 300, same-net through-holes change layer free,
  nets short-first, failed nets promoted, up to 3 rip-up passes, best pass kept. Must be
  deterministic. Bar: every realistic test board (LED, 555 blinker, antenna, 4-layer) routes 100 %;
  the spike's random 30-part stress board must route at least 26/40 (no regression).
  `shortcut: grid maze router, upgrade to a shape-based router when real boards leave nets unrouted`.
- Pours: port `spikes/pcb/src/pour.rs` with the crate `i_overlay` 4.5 (planar booleans in ~1 ms;
  cadrum has no 2D face booleans and its 3D workaround took 1.3 s). Board − clearance-expanded
  foreign copper − keepouts − drills, 0.3 mm thermal spokes for same-net pads, subtract foreign
  copper again after spokes, islands under 1 mm² removed, clearance arcs circumscribed (64 segments).
- Vias never sit inside an SMD pad (solder wicks into an open via): fan out a short stub first.
  The step 4 review found 4 of 5 vias on the 555 board inside pads. `pcb_drc` reports
  `via in pad: via <net> in <ref>.<pin>`.
- Placement: parts with `at` stay; others are packed on the free board area, connected parts near
  each other, courtyards not overlapping, `edge` from the outline.
- Schematic SVG: `shortcut: grid of symbols with net labels instead of drawn wires, upgrade to
  wire routing when users ask for readable schematics`.
- 3D: board slab + package boxes through cadrum → `.step`; mesh → `.glb` with cadrum's
  `Mesh::write_gltf_binary` after mapping `[x,y,z]` to `[x,z,-y]` (positions and normals), in mm,
  as `spikes/pcb/src/fab.rs` does. Gerber writer and readback also start from `fab.rs`.
- Never hold the cadrum `kernel()` lock across `.await`. Long CPU work runs in `spawn_blocking`.

### 3.5 Errors (word for word)

- `source must be a .pcb.json file`
- `<source> does not exist; write it with write_file first`
- `name may only use letters, digits, - and _`
- `<ref>.<pin>: <ref> (<footprint>) has pins <list>`
- `unknown footprint <x>; use one of: <list>`
- `<ref> is off the board`
- `parts overlap: <a> and <b>`
- `unroutable: net <net> (<pin> to <pin>)`
- `board <name> is not built; run pcb_build first` (pcb_drc)

`pcb_drc` reads `pcb/<name>.circuit.json` (so a test can break a built board by editing it) and the
rules from `pcb/<name>.pcb.json`. Report lines, one per fault, sorted, also written to
`fab/<name>/<name>-drc.txt`:
- `clearance: <item> to <item> is <gap> mm, needs <rule> mm` (items like `R1.1`, `track GND`, `via A`)
- `track width: track <net> is <w> mm, needs <rule> mm`
- `edge: <item> is <gap> mm from the board edge, needs <rule> mm`
- `unrouted: net <net> (<pin> to <pin>)`
- last line `N DRC error(s).` or `No DRC errors.`

### 3.6 Test hooks

- `forge_server::tools::pcb::footprint(name: &str) -> Result<Footprint, String>`, where
  `Footprint { pads: Vec<Pad>, courtyard: [f64; 4], height: f64 }` and
  `Pad { number: String, at: [f64; 2], size: [f64; 2], drill: Option<f64> }` (all fields `pub`;
  courtyard is `[min_x, min_y, max_x, max_y]`). `fp_` tests use it.
- `spec_` tests call `pcb_build` on a `.pcb.json` and check the 3.5 errors; step 2 wires parsing and
  validation into `pcb_build` (`.tsx` keeps using Node). Everything else goes through the tools.

## 4. Models: who does what

| Role | Model | `delegate_task` target |
|---|---|---|
| Spike and code | GPT-6.1-Sol | codex `gpt-6.1-sol`, options `{"reasoningEffort":"high","serviceTier":"default"}` |
| Tests | Gemini 3.8 Flash | antigravity `gemini-3.8-flash-high` |
| Review, check, version, commit | Orchestrator (Claude Opus 5.5) | — |

Use a distinct `clientRequestId` per round. If a step fails review, send a new round with the brief,
the findings and the agent's previous report.

## 5. Work order

- [x] Step 0: spike (GPT)
- [x] Step 1: reference tests (Gemini)
- [x] Step 2: spec parser, footprints, netlist check, placement (GPT)
- [x] Step 3: autorouter, pours, keepouts, antennas (GPT)
- [x] Step 4: outputs: Gerber, drill, pnp, BOM, SVGs, circuit.json, board.json, GLB, STEP (GPT)
- [x] Step 5: native pcb_drc, delete pcb_export_fab, rf_calc calculators (GPT)
- [x] Step 6: retire the Node engine and the .tsx format (GPT)
- [ ] Step 7: release 0.17.0 and finish line (orchestrator)

### Step 0 brief: spike (send to gpt-6.1-sol)

> You work on Forge, a Rust app. Read `AGENTS.md`, then `plan.md` sections 1 to 3. Work only in a
> new crate `spikes/pcb/` (add it to nothing; build with
> `cd spikes/pcb && CARGO_TARGET_DIR=../../target cargo run --release`). It may depend on `cadrum`
> 0.9 like `spikes/occt`. Prove each item with printed numbers:
> 1. IPC-7351B nominal land patterns for 0402, 0603, soic8 and qfn32: print pad size and pad centre
>    spacing next to KiCad's from `/usr/share/kicad/footprints` (parse the `.kicad_mod` pad lines).
> 2. Grid A* router per plan.md 3.4 on the LED board of section 3.1, and on a generated 30-part,
>    2-layer, 50×40 mm board with 40 two-pin nets. Report routed/unrouted counts, vias and time.
> 3. A ground pour on the bottom layer of the LED board with clearance and thermal reliefs. Try
>    cadrum 2D faces first; if it is too slow (>1 s) or wrong, try `i_overlay` and compare.
> 4. Write Gerber X2 for F_Cu and Edge_Cuts and an Excellon drill file; render all three to one SVG
>    with your own minimal Gerber reader so a human can check them.
> 5. Board slab + part boxes through cadrum to STEP and to GLB (find whether Forge already has a
>    GLB writer: `grep -rn glb server/src`).
> Save the output to `spikes/pcb/results.log`. Write `spikes/pcb/README.md`: what works, timings,
> the polygon choice and why, and any number in plan.md that is wrong, with arithmetic. Do not
> touch `server/`. Report the log and the decisions.

**Orchestrator check:** read the log and SVG; check the 4 footprints against KiCad by hand; record
the polygon decision and any new crate (with its reason) in section 3.4; copy pitfalls into later briefs.

### Step 1 brief: reference tests (send to gemini-3.8-flash-high)

> You write tests for Forge, a Rust server in `server/`. Read `AGENTS.md`, `plan.md` sections 2 and 3,
> `spikes/pcb/README.md` and `server/tests/board.rs` (style to follow). Section 3.6 says which API each
> group calls. `footprint()` does not exist yet: put the `fp_` tests in `mod fp` marked
> `#[cfg(any())] // step 2 removes this line` so the file compiles; every other test goes through
> the tools and must compile now. Create
> `server/tests/pcb_native.rs`. Do not edit any other file.
>
> Write one test per Reference bullet, per output file and report line in 3.3, per error in 3.5, and
> per `rf_calc` op in section 2 (use published values: IPC-2152 chart, Onderdonk formula, IPC-2221B
> table B, E96). Build boards by writing `pcb/<name>.pcb.json` and calling `pcb_build`. Include the
> LED board, a ground-pour board (`fab/<name>/<name>-B_Cu.gbr` contains `G36`), the antenna board
> from `pcb_build_accepts_a_printed_antennas_open_arm…` translated to JSON (same max-y assertion),
> a 555 LED blinker (soic8 NE555, 3 resistors, 2 capacitors, LED, 2-pin header) that must route with
> no errors, a 4-layer board, an auto-placed board with no `at`, and `pcb_drc` on a clean and on a broken board.
> Parse Gerber/Excellon text directly; check every JSON number the UI reads is JS-formatted.
> Mark every test `#[ignore = "stage 3 not built yet"]`, with name prefixes `spec_`, `fp_`, `route_`,
> `pour_`, `out_`, `drc_`, `calc_` so each step un-ignores its group. No network, no Node, no KiCad.
> Run `cargo test --workspace`: everything else passes and yours are ignored. Report the names and tail.

**Orchestrator check:** only `server/tests/pcb_native.rs` changed; every bullet and error has a test;
numbers match the plan and spike; the suite passes with the new tests ignored.

### Step 2 brief: spec, footprints, netlist, placement (send to gpt-6.1-sol)

> You code Forge, a Rust server in `server/`. Read `AGENTS.md`, all of `plan.md` and
> `spikes/pcb/README.md`. Create `server/src/tools/pcb/` (`mod.rs`, `spec.rs`, `footprint.rs`,
> `place.rs`): parse and validate `.pcb.json` per 3.1 with the errors of 3.5, generate the footprints
> of 3.2, build the netlist, check pins, place parts per 3.4. Keep it out of the outputs for now;
> expose pure functions plus the 3.6 hooks, and make `pcb_build` on a `.pcb.json` parse and validate
> it (returning the 3.5 errors; output comes in step 4). Un-ignore the `spec_` and `fp_` tests and make them pass without changing
> what they assert. No new crates unless step 0 chose one. Done means `cargo test --workspace` passes
> and `cargo clippy --workspace` shows no warnings. Report the diff summary and test tail.

**Orchestrator check:** read the diff; spot-check two footprints against KiCad; clippy clean.

### Step 3 brief: router, pours, keepouts, antennas (send to gpt-6.1-sol)

> Same rules as step 2. Add `route.rs`, `pour.rs`, `antenna.rs` to `server/src/tools/pcb/` per 3.4,
> using the step 0 spike code and its README pitfalls. Port the 5 antenna shapes from tscircuit
> (MIT) and add tscircuit to the third-party notices file. Un-ignore `route_` and `pour_`. Routing
> runs in `spawn_blocking`. Report routing time for the 30-part board.

**Orchestrator check:** rerun tests 3 times (router must be deterministic: same input, same output);
view the LED and antenna board SVGs.

### Step 4 brief: outputs (send to gpt-6.1-sol)

> Same rules as step 2. Add `gerber.rs`, `svg.rs`, `export.rs`; write every output in 3.3 and the
> report text. Switch `pcb_build` in `server/src/tools/board.rs` to the native engine when `source`
> ends with `.pcb.json`; keep the `.tsx` path working unchanged. Update the `pcb_build` description to
> document the JSON format (keep it under the current length). Un-ignore `out_`. Then check by hand that
> `rf_antenna_sim`'s reader accepts the new `circuit.json` (read `server/runners/openems_antenna.py`).
> Update `docs/ARCHITECTURE.md`.

**Orchestrator check:** open the outputs in the app (`run` skill): board view, schematic, 3D,
`design_assemble` with an enclosure. Load the Gerbers in KiCad's GerbView by hand once.

### Step 5 brief: pcb_drc and rf_calc (send to gpt-6.1-sol)

> Same rules as step 2. Rewrite `pcb_drc` in `server/src/tools/pcb.rs` natively per section 2 and
> 3.5 (input `{"board":"<name>"}`, reads what `pcb_build` wrote). Delete `pcb_export_fab` and its
> tests. Add the `rf_calc` ops of section 2 in `server/src/tools/rf.rs` / `rfmath.rs`. Un-ignore `drc_`
> and `calc_`. Update the agent prompt in `server/src/agent.rs` and `docs/ARCHITECTURE.md`.

**Orchestrator check:** DRC finds the faults in the broken test board and nothing on the clean one;
calc numbers match the cited sources; `every_tool_schema_is_portable` passes.

### Step 6 brief: retire the Node engine (send to gpt-6.1-sol)

> Same rules as step 2. Remove the `.tsx` path: delete `engines/pcb/`, `run_engine`, `pcb_engine`,
> `node_program` (setting, `ToolContext` field and env parsing) if nothing else uses them, the
> `npm run pcb-engine` script, the Node tests in `server/tests/board.rs`, and Node from the setup
> script and desktop bundle (`scripts/stage-app.sh`). Error for a `.tsx` source:
> `source must be a .pcb.json file`. Update `docs/ARCHITECTURE.md` and the ROADMAP dependency table.
> Done means `cargo test --workspace`, `npm run typecheck` and clippy pass.

**Orchestrator check:** `grep -rn "tscircuit\|pcb-engine\|kicad-cli\|node_program" server desktop scripts web`
shows only notices/history; app builds with `sh scripts/stage-app.sh`.

### Step 7: release 0.17.0 and finish line (orchestrator)

Bump to 0.17.0 (package.json, `server/Cargo.toml`), CHANGELOG entry, tick ROADMAP Phase 1 PCB items.
Finish line: with real Claude in the app, ask "design a USB-powered LED blinker board with a 555 and a
mounting hole in each corner, then an enclosure for it". It must build, route with no errors, pass
`pcb_drc`, and fit in the enclosure in `design_assemble`.

## 6. Pitfalls (already inside the briefs; listed here for the reviewer)

- Keep JS-style number formatting and key order in every JSON the UI reads.
- Error strings word for word; the prompt and tests rely on them.
- Router and pours must be deterministic (no HashMap iteration order in output).
- Never block the async runtime; never hold `kernel()` or any `std::sync::Mutex` across `.await`.
- Tools never touch paths outside the project: every path through `project_path`.
- The `.tsx` path keeps working until step 6, so the app never loses PCB support mid-stage.
- Footprint pads use KiCad's frame (y down, pin 1 at -y), verified equal to KiCad in step 2. Outputs
  are +y up: flip y once when writing, and make pick-and-place rotations match JLCPCB's convention
  for KiCad footprints. Format every coordinate through `jsfmt` (raw f64 shows `-0.9750000000000001`).
