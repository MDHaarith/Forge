# Forge roadmap

## Goal: Forge stands on its own

Forge must run on a machine with no FreeCAD, KiCad, Blender, OpenSCAD, openEMS, iverilog, yosys or Node.
Every capability is built into Forge. Embedded libraries compiled into the binary (for example a CAD kernel
such as OpenCascade) count as Forge; separately installed programs do not.

shortcut: today several tools still call external programs (table below); replace them in the order of
the phases, and keep the external version working until its native replacement passes the same tests.

### What still depends on an outside program (v0.10.0)

| Tool | Needs | Native replacement |
|---|---|---|
| freecad_build, freecad_inspect, freecad_drawing | FreeCAD | CAD kernel tools |
| blender_build | Blender | renderer and mesh modifiers |
| openscad_render | OpenSCAD | fold into the CAD kernel, or drop |
| pcb_build | Node + tscircuit engine | Rust PCB engine |
| pcb_drc, pcb_export_fab | kicad-cli | native DRC, Gerber/Excellon writer |
| chip_simulate, chip_synthesize | iverilog, yosys | Verilog simulator and synthesis |
| rf_antenna_sim | openEMS | FDTD solver |
| git_status, git_commit, git_log | git | built-in git library (gitoxide) |

Already native: list_files, read_file, write_file, cad_model, rf_calc, design_assemble.

## Phases (estimates include tests; ±50%)

**Phase 1, core (~35k lines):** Forge works with nothing else installed.
- CAD kernel tools: sketch, extrude/revolve, booleans, fillets/chamfers, ISO holes, STEP/STL export,
  mass properties, hole detection, interference (port `server/runners/forge_cad.py` helpers).
- Rust PCB engine: netlist, IPC-7351 footprint generator, placement, autorouter, copper pours,
  schematic SVG, 3D board; native DRC; Gerber, drill, pick-and-place, BOM.
- Renderer: CPU path tracer (PBR), glTF export, mesh modifiers (subdivide, bevel, array, mirror, solidify).

**Phase 2, common (~30k lines):** drawings (hidden-line views, dimensions, SVG/DXF/PDF), assembly
joints, FEM (volume mesh + linear-static solver), CAM (2.5D toolpaths + G-code), SPICE simulator,
FDTD antenna solver, animation output, web viewers for FEM results and G-code.

**Phase 3, niche (~20k lines):** Verilog simulator and synthesis, KiCad file export for exchange,
OpenSCAD interpreter, BIM/IFC.

Target: about 57 tools in total. More than ~60 tools makes the AI pick badly; group features into
workflow tools instead of one tool per app command. `docs/reference/app-features.md` lists every
FreeCAD, KiCad and Blender command as the checklist of what to cover.

## Accuracy rule

Solvers (autorouter, FEM, FDTD, SPICE) must be checked against published reference results in tests
before they are trusted for real hardware. Keep the reference numbers in `server/tests/fixtures/`.

## Other planned work

1. Approvals: confirm before tools that write outside `models/`, run commands, or export for manufacture.
2. Desktop app as a Flatpak.
3. Cross-domain links: firmware pin map from the schematic; enclosure generated from the board.
4. Load third-party MCP servers as capabilities.
5. Prompt caching, a Stop button, and history compaction for long chats.

## Known shortcuts in the code

Run `grep -rn "shortcut:" server/src desktop/src` for the list of deliberate limits and when to fix them.
