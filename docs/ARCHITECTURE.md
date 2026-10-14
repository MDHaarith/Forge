# Forge architecture

Forge is a local AI workshop. A Rust server runs an AI agent that builds real files (CAD models, drawings,
PCBs, renders) inside a project folder, and a web UI shows the chat, the files and 3D previews. The desktop
app (Tauri) runs the same server in-process.

## How a chat message flows

```
web/main.ts  ──POST /api/projects/<name>/chat──▶  server/src/http.rs
                                                    │  (localhost Host check, JSON only, 1 MB, 409 while busy)
                                                    ▼
                                              projects.rs  chat_started()
                                    ┌───────────────┴────────────────┐
                     provider = claude                        any other provider
                                    ▼                                ▼
                       claude_code.rs: spawns `claude -p`     agent.rs: model ⇄ tools loop
                       with Forge's MCP server                (model.rs talks to OpenAI-compatible APIs
                                    │                          or the offline demo)
              Claude calls tools ──▶ mcp.rs  POST /mcp/<name>        │
                                    └───────────────┬────────────────┘
                                                    ▼
                                    tools/mod.rs  all_tools() → one Tool per capability
                                    (name matched by agent.rs find_tool, input checked by tools/validate.rs,
                                     paths by tools/paths.rs)
                                                    ▼
                         NDJSON events stream back: user, text, tool_call, tool_result{files}, error, done
                         and are saved in <FORGE_HOME>/.state/<project>.json
```

## Files

```
server/src/
  main.rs                  standalone binary: PORT, app folder, starts serve()
  lib.rs                   Settings (all configuration, built once from env) and serve()
  env.rs                   loads ~/.config/forge/env
  http.rs                  every /api route, static UI from dist/, security checks
  projects.rs              project folders, git init, saved history, busy flag, Claude session id
  agent.rs                 Forge's own agent loop, system prompt (names only available tools),
                           execute_tool and find_tool (shared with mcp.rs), AgentEvent types
  model.rs, providers.rs   OpenAI-compatible providers, offline demo, provider/model switching
  claude_code.rs, mcp.rs   Claude via the user's Claude Code CLI; tools served to it over MCP
  open.rs, platform.rs     "Open in app" buttons; distro detection and install hints
  jsfmt.rs                 JavaScript-style number/JSON formatting (the UI expects it)
  tools/
    mod.rs                 Tool struct, all_tools() registry, availability, describe_tools()
    paths.rs               project_path(): the only way a tool may turn input into a path
    exec.rs                run a program: no shell, timeout + kill, 32 MB output cap
    validate.rs            JSON-schema subset used to check tool input (type, enum, anyOf,
                           additionalProperties:false, min/maxItems)
    launch.rs              run an app natively or as a sandboxed Flatpak
    workspace.rs vcs.rs code.rs              files, git, run_command
    cad.rs                                   openscad_render
    part.rs                                  part_build: OCCT solids, booleans and engineering parts → STEP/binary STL/JSON
    part/ops.rs                              outlines, extrude/revolve, edge selectors, fillet/chamfer, place
    part/shapes.rs                           loft, tangential sweep, inward shell, mirror and linear/polar patterns
    part/threads.rs                          ISO coarse thread rods, shared internal-thread cutters and fine bounding-box meshes
    part/sheet.rs                            bent sheet sections and flat length/width metadata
    part/kit.rs                              standard holes, grid/corner patterns, rounded enclosure/lid/standoffs
    part/standards.rs                        ISO hole/head/insert/pitch tables and material densities
    part/holes.rs                            analytic STEP hole grouping and standard diameter descriptions
    part/inspect.rs                          part_inspect: STEP/STP/STL size, volume, centre of mass, holes, configurable overhangs and STEP pair interference/clearance
    part/drawing.rs                          part_drawing: scaled landscape SVG views, hidden lines, dimensions, hole table/title block and top-outline R12 DXF
    script3d.rs                              blender_build and shared runner-output helpers
    board.rs pcb.rs design.rs fit.rs         native PCB build/exports, move/lock/unlock edits, footprint import, DRC and board-in-enclosure fit
    pcb/spec.rs                             strict native .pcb.json parser, pin aliases, netlist and rule overrides
    pcb/footprint.rs                        pure nominal/KiCad footprint dimensions, rounded SMD pads, courtyard, silk and body geometry
    pcb/kicad.rs                            bounded s-expression reader, modern/legacy .kicad_mod pads and outline bounds → y-up native footprint JSON
    pcb/place.rs                            deterministic placement, rotated courtyards, polygon edges and hole checks
    pcb/drc.rs                              saved-copper geometry, 4 mm spatial index, pour/via connectivity and sorted DRC faults/ERC warnings
    pcb/route.rs                            regional tree A*, PathFinder, fixed differential pairs, locked-copper component routing, physical via spans, planes and filled vias-in-pad
    pcb/beautify.rs                         deterministic octilinear pulling, corner chamfers, via reduction and centred pad entries before tuning
    pcb/impedance.rs                        IPC-2141 single/differential microstrip widths and shared report/drawing target text
    pcb/tune.rs                             shared lengths/coupling metrics, continuous-clearance meanders, skew bumps and teardrops
    pcb/pour.rs                             i_overlay planar copper fills, pad/via thermals, foreign-via antipads and hole contours
    pcb/antenna.rs                          five MIT tscircuit arm geometries, feed pads and feed-opening keepouts
    pcb/gerber.rs                           Gerber X2 layers, clear pour-hole regions, RoundRect flashes, reduced/windowpane paste, contour/fiducial helpers and split Excellon
    pcb/ipc356.rs                           fixed-column metric IPC-D-356A netlist, aliases, ordered pad/via test points
    pcb/fab.rs                              A4 fabrication SVG, dimensions/origin, drill symbols/table, physical stackup and manufacturing notes
    pcb/panel.rs                            stepped manufacturing layers, polygon tab cutouts, mouse-bites, rails, fiducials/tooling, V-score and suffixed PNP
    pcb/svg.rs                              shared SVG escaping/paths, rounded PCB pads and connected grid schematics with orthogonal A*, junctions and power symbols
    pcb/export.rs                           rounded JS JSON, JLCPCB CSVs and shared-kernel STEP/GLB assemblies with exact two-arc slot cuts
    rf.rs rfmath.rs                          RF and PCB calculators (IPC-2221/B, Onderdonk, E-series); rf_antenna_sim (openEMS)
    circuit.rs                               native circuit_simulate tool, project-relative .cir input and async artifact writes
    circuit/parse.rs                         bounded case-insensitive SPICE reader, model scopes, nested subcircuit flattening, waveforms and analyses
    circuit/devices.rs                       Shockley/Gummel-Poon/level-1 MOS equations, analytic current/charge derivatives and junction limiting
    circuit/solve.rs                         MNA, cached pivoted dense LU, Newton/gmin/source stepping, adaptive trapezoidal/BE transient integration
    circuit/output.rs                        operating-point tables, deterministic CSV, escaped standalone SVG plots and concise tool summaries
    chip.rs                                  Verilog simulate/synthesize
server/runners/*.py        Blender and openEMS scripts; forge_art.py = visual modelling helpers
server/tests/              integration tests (common/ has TempDir, fake Claude, tool helpers)
  pcb_batch6.rs            locked-copper geometry/connectivity/DRC, deterministic exports, KiCad parsing and import path/order tests
  circuit_sim.rs           analytic passive/semiconductor/charge references, parser/errors/path rules, determinism and 200-node debug performance
  fixtures/sim/*.json      independently derived circuit reference values with source/formula descriptions
desktop/src/main.rs        Tauri app: binds a free port, runs forge_server::serve in-process
web/                       UI (TypeScript + Vite + three.js): main.ts app, design.ts PCB/3D view, viewer.ts,
                           markdown.ts (Markdown + KaTeX, sanitized with DOMPurify)
licenses/tscircuit-MIT.txt  licence for the ported printed antenna shapes and keepouts
scripts/                   setup-fedora.sh installer, stage-app.sh bundles files for the desktop app
docs/                      this map, ROADMAP.md, reference/ (engineering CAD, KiCad and Blender feature checklists)
```

`pcb.rs` remains the PCB module root and re-exports the native pure helpers. `pcb_build` validates,
places, routes and pours `.pcb.json` inputs in `spawn_blocking`. The module root writes the report,
circuit JSON and board boxes, then collects every SVG, CAD and manufacturing export. Native builds
are always available without an external program. CAD export shares
`part.rs`'s kernel lock, held only inside the worker;
GLB coordinates, normals and edges use `[x,z,-y]` for Forge's viewer. All output paths resolve through
`project_path` before async writes. Routing grids are limited to 65 million cells; obstacle bitsets
and regional search arrays bound memory use.
Only `.pcb.json` inputs are accepted; other formats fail with `source must be a .pcb.json file`.
`board.rs::board_source` shares source/path validation with `pcb_build`, `edit` and footprint import.
`POST /api/projects/<name>/board` accepts `{source, move:{ref,at,rotation?,side?}}`, `{source,lock:net}`
or `{source,unlock:net}`. `http.rs` applies `read_json` and reserves the session's agent busy guard;
an active turn returns 409 `agent is running`. `board.rs::edit` preserves JSON key order/unknown fields,
rounds moves to 0.01 mm, removes affected locked copper, and calls `pcb_build`. Success returns
`{report,unlocked}`; failed builds return 422 `{error}` while retaining the edited spec.
Optional spec `tracks` and `vias` become `route::Board.fixed` and `fixed_vias` in `from_prepared`.
`Grid::connect_locked` connects only missing copper components; beautification and tuning operate on
new routes. Fixed vias join final routes after tuning so all exports include them. Locked tracks keep
normal trace JSON, while intrinsic antenna copper retains its existing metadata.
Length matching counts locked copper without changing it; STEP/GLB adds locked copper solids and via barrels.
`pcb_import_footprint {file,board,name?}` uses `pcb/kicad.rs::parse` to insert native inline `footprints`.
`footprint.rs::native` validates pads and converts y-up JSON to the existing internal KiCad frame;
built-in name clashes are errors. Pad rotations, top/bottom/through layers, rounded pads, oval drills
and nonplated holes reach the same router and exports. Custom primitives and courtyard/body polygons
use bounding rectangles; s-expression nesting is capped at 128.
`web/main.ts` reads part origins/rotations from circuit JSON without changing legacy board-box files.
`showBoardSvg` maps native and legacy SVG coordinates, supports snapped part dragging, ghost boxes,
R/F/Escape, and sends `onEdit` callbacks to the board endpoint. Net controls show current nets and
locked track nets; successful edits reload SVG/JSON/3D views and show reports in the Assistant log.
`pcb_drc {"board":"<name>"}` reads the saved native circuit and source rules, checks geometry and
connectivity in `spawn_blocking`, and writes sorted faults to `fab/<name>/<name>-drc.txt`.
Electrical rules use the expanded source netlist: single-pin nets, unconnected or shorted two-pad
passives/diodes/crystals, and parts with no connected pads (except antennas). Reports list sorted DRC
faults, sorted ERC warnings, the optional filled-via note, the optional `N ERC warning(s).` count,
then the existing DRC summary. ERC warnings never count as DRC errors. Old circuit JSON without
`rect_border_radius` remains valid.
Net classes may override clearance as well as trace width; the router, pours and DRC share those rules.
`rules.via_in_pad` is a strict boolean, default false. When true, a via may overlap only its own
net's SMD pads; all other clearance checks remain active. The DRC report notes that filled/capped
vias-in-pad are required when this rule is enabled.
Drill minima and annular-ring minima come from `rules.via` (drill and half the diameter/drill difference).
Saved-coordinate checks allow 0.000001 mm rounding tolerance. Through-hole courtyards reserve both sides.
Routing reserves short SMD exits across three deterministic passes, then repairs missing nets by
ripping up their blockers. Multi-pin branches grow from the whole connected tree. Large-board A*
starts within a 3 mm margin around the terminals/tree and doubles the window on failure, up to the
whole board. Search costs and 32-bit parents occupy only that window. Static obstacle bitsets are
shared by trace width/clearance with per-net exemptions and a 128 MiB cache budget. Independent
rule-class masks are prepared with scoped standard-library threads; fixed net priorities and search
ties make results independent of worker count. Negotiation uses 16-bit occupancy counts, computed
lazily in visited 128 × 128 cell tiles on large grids, with sparse history, continuous clearance checks
and deterministic restart orders. Large same-net via sets are indexed by grid column for layer changes.
Blocked terminal cells fail before allocating search arrays.
Inner pours reserve their layers for planes: signals stay on other layers, foreign through vias get
antipads, and each plane terminal reaches copper through its plated hole or a short SMD escape/via.
Outer pours retain trace-tree routing. Build diagnostics log routing duration, completion, vias and
passes. Gerber pours emit clear-polarity hole regions before pads, tracks and vias, preserving antipads
and thermal gaps in manufacturing copper. Pour thermals join both pads and vias; saved-copper DRC checks
connectivity through the actual pour contours. DRC indexes copper bounds in 4 mm buckets before
exact geometry checks and uses saved placements even when source rules change.
`server/tests/pcb_native.rs` includes ignored 200-part tool-level benchmarks with a deterministic
5 × 4 cluster layout, unique pins, 75% chip budgets, filled vias and budget-limited neighbour buses.
The 4-layer benchmark requires every net routed and clean saved-copper DRC; the 2-layer variant
requires at least 95% completion. Both print metrics and save copper/SVG review artifacts under
`/tmp/batch3/board4` or `/tmp/batch3/board2`.
Batch 4 fields are optional: `board.er`/`prepreg`, impedance net classes, `diffpairs`,
`length_groups`, `rules.blind_vias` and `rules.teardrops`. Net classes choose trace width or impedance.
`route::depth` maps logical indices (top, bottom, inner1, inner2) to physical stack depth.
Via spans filter copper, mask, pours, clearance and saved JSON without changing through-via ordering.
Blind/buried vias get physical-layer-numbered drill files and partial-depth cuts in STEP/GLB.
Pairs route a midpoint centreline with pad escapes and miter clearance, then offset both traces and
spread paired vias. Coupled copper stays fixed through PathFinder; ordinary routing remains the
fallback if a pair or the remaining board cannot route, including pins with multiple lands.
Shared metrics measure saved track lengths
and parallel overlap for reports and DRC. Tuning runs before pours; teardrops run after tuning and
use the same continuous geometry as DRC. `server/tests/pcb_batch4.rs` checks each feature, validation,
saved-copper DRC and raw artifact hashes captured from 0.21.0; its hash comparison normalizes the
Gerber package-version metadata.
`rules.beautify` is a strict boolean, default true. After routing and before tuning, each ordinary net
normalizes overlaps and collinear chains, pulls clearance-clean octilinear paths tight, chamfers clear
right/acute corners, removes redundant non-plane vias and centres SMD entries. Pads, vias and junctions
anchor chains. Fixed antenna copper and all differential-pair nets stay untouched. Each accepted net
keeps pad connectivity and a 0.2 mm maximum total pad-entry length allowance; via counts cannot grow.
Four deterministic rounds and 256 pull candidates per chain bound polishing work. Channel centring
is deferred. `beautify:false` preserves the previous raw route; changed copper adds a build-report
line with segment counts, removed vias and chamfered corners. `server/tests/pcb_beautify.rs` checks
quality, independent saved-copper connectivity, DRC, repeatability, planes, pairs and length groups.
`pcb_export_fab` is removed because native `pcb_build` writes every manufacturing artifact.

Batch 5 always appends `fab/<name>/<name>.ipc` after the drill files and adds
`fab/<name>/<name>-fab.svg`. IPC records use 0.001 mm units, fixed columns, unique NNAME aliases,
part/pin order followed by route-order vias, physical via spans and mask accessibility.
The drawing shares the Excellon holes and rounded tool diameters, lists physical copper/dielectric
thicknesses and impedance report text, and notes filled vias, blind/buried spans and panel separation.
Slots in STEP/GLB use exact circular ends and straight sides through the board thickness;
existing slot-containing 3D files necessarily change because the bounding-cylinder geometry is corrected.
`rules.paste_reduction` (default 0) and `rules.paste_windowpane` (default false) affect only paste:
shrink each side/radius, omit openings below 0.1 mm, and window large SMD pads with 0.3 mm webs
and a 60% area target within the reduced opening. Large reductions cap the target at the available
area; disabled paste fields keep previous paste bytes. Validation retains the existing
`must be nonnegative` helper wording.
`panel` is optional: rows/cols 1–20, tabs or V-score, spacing, x/y/both rails, tab count/width,
mouse-bites, fiducials and tooling. Panels currently require rectangular board outlines; tab
geometry must leave a tab-width corner margin and fit all tabs. V-score requires zero spacing.
Tab counts are capped at 64 per edge to bound polygon and mouse-bite work.
Panel dimensions must fit Gerber 4.6 coordinates; rails must fit enabled fiducials/tooling.
`fab/<name>-panel/` mirrors every Gerber/drill layer, translating pads, tracks, fixed copper,
teardrops, pours and drill spans. Polygon differences leave closed routed gaps and tab bridges;
rail fiducials appear only on outer copper/masks, tooling and mouse-bites only in NPTH.
V-score adds `<name>-panel-VScore.gbr`; PNP adds `_r<row>c<col>` to each designator,
with rows ordered from bottom to top. The original BOM remains per board.
`server/tests/pcb_batch5.rs` checks panel output, compatibility, validation and tool-level writes;
module tests pin IPC columns, paste coverage, drill totals, stackup sums, tab bridges,
four-layer translations and analytic STEP slot volume. Batch 4 retains every old artifact hash.

Generated rectangular SMD pads use a 25% corner radius, capped at 0.25 mm; drilled rectangles stay
sharp. Circuit JSON places `rect_border_radius` immediately after `height` only for positive radii.
Copper, mask and paste flash a shared RoundRect aperture macro with rotated corner centres; mask
expansion also expands the radius. Routing, pours and clearance checks retain full rectangles.
TO-220, TO-92, SOT-23-6, SOT-89, SOD-323, SMA/SMB/SMC, MSOP-8/10 and microSD dimensions are copied
from the [official KiCad footprint library](https://gitlab.com/kicad/libraries/kicad-footprints).
Courtyards and bodies retain library bounding boxes in the existing rectangular geometry model;
SOT-89's custom middle pad is represented by its anchor and tab, both numbered 2. Diode numbering
maps KiCad's cathode pad 1 to Forge pad 2, preserving `K`/`A` aliases. microSD retains pins 1–10 and SH.

Schematics place parts by shared-net BFS, then pack symbols with wire gutters on a 2.54 mm grid.
Two-pad symbols are horizontal; other symbols use DIP pin order. Orthogonal tree A* avoids bodies,
foreign pins and wire vertices, allows perpendicular crossings without junctions, and marks only
same-net branches with junctions. Ground and supply nets use per-pin power symbols; signal nets
over eight pins or failed/bounded searches use per-pin labels. Search is limited to one million grid
cells. Unconnected pads carry NC crosses. Both SVG previews remain standalone.

## Native circuit simulation

`circuit_simulate {"source":"sim/<name>.cir"}` reads only through `project_path`, solves in
`spawn_blocking`, and resolves every output through `project_path` before writing. The one new
`pcb` domain tool is always available. It writes `sim/<name>/<name>-op.txt`, `-dc.csv`/`-tran.csv`
and matching standalone SVG plots. CSV contains all user/hierarchical node voltages and V/L/E
branch currents, positive from the first terminal to the second. Internal parasitic nodes are hidden.
`.print`/`.plot` selects up to eight SVG traces; CSV retains all columns. Tool text includes one
summary per analysis and the first 40 operating-point table lines.

The case-insensitive SPICE3 subset treats the first physical line as the title, accepts `*` comments,
`;` tails, `+` continuations, optional `.end`, `0`/`gnd`, engineering suffixes and trailing units.
It supports R, C/L with `ic=`, V/I with DC/PULSE/SIN/PWL, D, NPN/PNP Q, level-1 NMOS/PMOS M with
W/L, E/G, `.model`, and nested `.subckt`/X with scoped models and port mapping. Analyses are `.op`,
one `.dc source start stop step` and one `.tran tstep tstop [tstart [tmax]] [uic]` per deck.
`.ic v(node)=value` and `.options reltol abstol vntol gmin itl1 itl4` are supported. Unknown
directives/parameters fail; external includes and expression/parameter languages are not loaded.

Device currents and charges use analytic terminal derivatives. MNA uses partial-pivot dense LU,
reusing factors when the matrix is unchanged. Newton checks voltage/current changes and unlimited
KCL/KVL residuals, limits junction/FET voltages, and retries failed bias points with gmin then source
stepping. Transient integration uses trapezoidal charge/flux companions, backward Euler at startup
and after source breakpoints, and charge/flux LTE to accept/reject steps: step doubling for backward
Euler and third divided differences for trapezoidal integration.
PULSE/PWL corners, SIN delay, `tstart` and `tstop` are exact integration boundaries.

SPICE choices: zero/omitted PULSE rise/fall times use `tstep`; zero/omitted pulse width/period use
`tstop`; zero/omitted SIN frequency uses `1/tstop`. Explicit source DC values apply to `.op`/`.dc`,
while transient bias uses the waveform at zero. Capacitor/inductor `ic=` applies with `uic`;
without `uic`, `.ic` temporarily clamps the initial bias and releases it for integration.
Model defaults follow the [ngspice manual](https://ngspice.sourceforge.io/docs/ngspice-46-manual.pdf),
including the junction-capacitance continuation above FC=0.5. Temperature is fixed at 27 C.
Floating DC nodes and ideal voltage/inductor loops fail before matrix solving. Limits are 2000
unknowns, 1 MiB input, 20000 flattened elements, 32 hierarchy levels, 200000 transient attempts and
two million saved values. Dense LU should become sparse above roughly 500 nodes; streaming and
an explicit hierarchy stack are the upgrade paths for the other resource limits.

`server/tests/circuit_sim.rs` uses independent analytic fixtures in `server/tests/fixtures/sim/`
for divider/bridge, RC/RL/RLC, diode Lambert W and breakdown, rectification, Ebers-Moll/Gummel-Poon,
MOS square law/body effect, CMOS switching and semiconductor charge derivatives. It also checks
flattening, controlled sources, initial conditions, exact errors, path confinement, deterministic
artifacts and a 200-node RC ladder with at least 1000 accepted steps under five seconds in debug.

## HTTP API (all under /api, localhost only)

| Method | Path | Does |
|---|---|---|
| GET | /api/status | tools with availability, current model |
| GET | /api/models | providers and their models |
| POST | /api/model | switch provider/model and Claude thinking level `effort` (409 while a chat runs) |
| GET, POST | /api/projects | list, create |
| GET | /api/projects/<name>/files, /file?path=, /history | file tree, file contents, saved events |
| POST | /api/projects/<name>/open | open a file in its desktop app |
| POST | /api/projects/<name>/chat | run a turn; streams NDJSON events |
| POST | /api/projects/<name>/stop | end the running turn (`{"stopped":bool}`) |
| POST | /mcp/<name> | MCP JSON-RPC for Claude Code (only during a Claude turn) |

## Adding a tool

1. Pick the domain file in `server/src/tools/` (or add one and `pub mod` it in `mod.rs`).
2. Write a `Tool::new(domain, name, description, json!(schema), run_fn)`. The description is what the AI
   reads: say what it does, what it writes, and its limits.
3. In `run_fn`: get paths only through `project_path`, run programs only through `exec::run`, return
   `ToolResult { text, files }` with every file written (the UI refreshes and previews them).
4. If it needs an external program set `requires` / `app` and `install`, so it hides when missing.
5. Append it in `all_tools()` in `tools/mod.rs`. The agent, MCP, the UI capabilities list and input
   validation pick it up automatically.
6. Add a test in `server/tests/` (skip with `eprintln!("skipped: …")` if the program is missing).
7. Bump the version and add a CHANGELOG entry (see AGENTS.md).
