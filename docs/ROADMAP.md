# Forge roadmap

## Goal: Forge stands on its own

Forge must run on a machine with no CAD/EDA apps, Blender, openEMS, git or Node.
Every capability is built into Forge. Embedded libraries compiled into the binary (for example a CAD kernel
such as OpenCascade) count as Forge; separately installed programs do not.

shortcut: today several tools still call external programs (table below); replace them in the order of
the phases, and keep the external version working until its native replacement passes the same tests.

### What still depends on an outside program

| Tool | Needs | Native replacement |
|---|---|---|
| blender_build | Blender | renderer and mesh modifiers |
| rf_antenna_sim | openEMS | FDTD solver |

Stage 3 is complete (v0.17.0): PCB builds and DRC are native; neither Node nor KiCad is needed.
Done in v0.18.0: USB-C, Micro-B, JST-PH and barrel-jack footprints; slots and NPTH; PathFinder autorouter.
Done in v0.19.0: complex-board autorouter (tree growth, rip-up, restarts) and opt-in filled vias-in-pad.
Done in v0.20.0: wired schematics, ERC warnings, rounded pads, 11 more footprints.
Done in v0.21.0: deterministic 200-part routing, regional searches and inner reference planes.
Done in v0.22.0: differential pairs, length meanders, impedance widths, blind/buried vias and teardrops.
Done in v0.23.0: default-on router beautification with octilinear pulling, chamfered corners, fewer vias and centred pad entries.
Done in v0.24.0: panelization with tabs/mouse-bites or V-score, rails/fiducials/tooling, metric IPC-D-356A,
reduced/windowpane stencil paste, fabrication drawings and exact 3D slots.
Done in v0.25.0: interactive part editing, locked tracks/vias with incremental routing, and native KiCad `.kicad_mod` footprint import.
Done in v0.26.0: native SPICE subset with operating points, DC sweeps, adaptive transient analyses,
R/L/C, diode/BJT/level-1 MOS models, E/G sources, nested subcircuits and analytic reference fixtures.
Done in v0.27.0: native AC small-signal and noise analyses, temperature/model scaling and resistor
TC1/TC2, Bode/noise plots, and offline ngspice cross-checks with an optional live fixture replay.
Done in v0.28.0: gitoxide project creation, status, staging, commits and logs; all project history
works without an installed git program. Retire the digital logic domain and redundant scripted CAD renderer;
exact engineering parts use `part_build`.
Next: the renderer (Blender replacement).

Already native: list_files, read_file, write_file, git_status, git_commit, git_log, part_build, part_inspect, part_drawing, rf_calc, design_assemble,
pcb_build (including manufacturing exports), pcb_import_footprint, pcb_drc and circuit_simulate. `pcb_export_fab` is removed because
`pcb_build` writes those files.

CAD kernel (`plan.md` stages 1 and 2 done): `part_build` makes exact solids (primitives, extrude,
revolve, loft, sweep, shell, mirror, linear/polar patterns, ISO threads, sheet metal, booleans,
fillet, chamfer, place, ISO holes, enclosure, lid, standoffs) → STEP/STL/JSON. `part_inspect`
reports size, volume, centre of mass, material mass, holes, configurable print overhangs and
STEP interference/clearance with `against`. `part_drawing` writes scaled SVG drawings with
hidden lines, dimensions, hole tables and title blocks, plus top-outline DXF. Exact CAD tools
now use the built-in kernel; the external CAD tools, runners and settings are removed.
`cad_model` is removed. PDF drawings remain planned.

## Phases (estimates include tests; ±50%)

**Phase 1, core (~35k lines):** Forge works with nothing else installed.
- CAD kernel tools: sketch, extrude/revolve, booleans, fillets/chamfers, ISO holes, STEP/STL export,
  mass properties, hole detection, interference and SVG/DXF drawings are native (stage 2 done).
- [x] Rust PCB engine (stage 3, v0.17.0): netlist, IPC-7351 footprint generator, placement, autorouter, copper pours,
  schematic SVG, 3D board; native DRC; Gerber, drill, pick-and-place, BOM.
  Stage 3 steps 2–3 add strict `.pcb.json` validation, generated footprints, pin/net checks,
  deterministic placement and 2/4-layer routing, keepouts, thermal copper pours and five printed
  antenna shapes. Step 4 adds PCB/schematic SVGs, GLB/STEP assemblies, circuit JSON, board boxes,
  all Gerber layers, split drills and assembly CSVs. Step 5 adds native DRC, SMD via fanout
  and PCB calculators. Step 6 retires the Node engine; only `.pcb.json` inputs are accepted.
- Renderer: CPU path tracer (PBR), glTF export, mesh modifiers (subdivide, bevel, array, mirror, solidify).

**Phase 2, common (~30k lines):** drawings (hidden-line views, dimensions, SVG/DXF/PDF), assembly
joints, FEM (volume mesh + linear-static solver), CAM (2.5D toolpaths + G-code), further SPICE models,
FDTD antenna solver, animation output, web viewers for FEM results and G-code.

**Phase 3, niche:** KiCad file export for exchange, BIM/IFC.

Target: about 57 tools in total. More than ~60 tools makes the AI pick badly; group features into
workflow tools instead of one tool per app command. `docs/reference/app-features.md` lists every
engineering CAD, KiCad and Blender feature as the checklist of what to cover.

## Accuracy rule

Solvers (autorouter, FEM, FDTD, SPICE) must be checked against published reference results in tests
before they are trusted for real hardware. Keep the reference numbers in `server/tests/fixtures/`.
Circuit AC, noise, temperature, DC and transient results now compare against stored ngspice-41 decks
under `server/tests/fixtures/sim/ngspice/`; normal tests require no external simulator.

## Other planned work

1. Approvals: confirm before tools that write outside `models/`, run commands, or export for manufacture.
2. Desktop app as a Flatpak.
3. Cross-domain links: firmware pin map from the schematic; enclosure generated from the board.
4. Load third-party MCP servers as capabilities.
5. Prompt caching, a Stop button, and history compaction for long chats.

## Known shortcuts in the code

Run `grep -rn "shortcut:" server/src desktop/src` for the list of deliberate limits and when to fix them.
