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
    board.rs pcb.rs design.rs fit.rs         native PCB build/exports, DRC, board-in-enclosure fit
    pcb/spec.rs                             strict native .pcb.json parser, pin aliases, netlist and rule overrides
    pcb/footprint.rs                        pure nominal/KiCad footprint dimensions, rounded SMD pads, courtyard, silk and body geometry
    pcb/place.rs                            deterministic placement, rotated courtyards, polygon edges and hole checks
    pcb/drc.rs                              saved-copper geometry, 4 mm spatial index, pour/via connectivity and sorted DRC faults/ERC warnings
    pcb/route.rs                            regional tree A*, shared bitsets, parallel obstacle preparation, PathFinder, inner-plane escapes and filled vias-in-pad
    pcb/pour.rs                             i_overlay planar copper fills, pad/via thermals, foreign-via antipads and hole contours
    pcb/antenna.rs                          five MIT tscircuit arm geometries, feed pads and feed-opening keepouts
    pcb/gerber.rs                           Gerber X2 layers, explicit clear pour-hole regions, RoundRect flashes and split PTH/NPTH Excellon
    pcb/svg.rs                              rounded PCB pads and connected grid schematics with orthogonal A*, junctions and power symbols
    pcb/export.rs                           rounded JS JSON, JLCPCB CSVs and shared-kernel STEP/GLB assemblies
    rf.rs rfmath.rs                          RF and PCB calculators (IPC-2221/B, Onderdonk, E-series); rf_antenna_sim (openEMS)
    chip.rs                                  Verilog simulate/synthesize
server/runners/*.py        Blender and openEMS scripts; forge_art.py = visual modelling helpers
server/tests/              integration tests (common/ has TempDir, fake Claude, tool helpers)
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
`pcb_export_fab` is removed because native `pcb_build` writes every manufacturing artifact.

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
