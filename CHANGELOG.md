# Changelog

## 0.12.0 — 2026-10-13

- New `part_build` tool: exact solids from Forge's own CAD kernel (OpenCASCADE via the cadrum crate),
  no FreeCAD needed. Ops: box, cylinder, sphere, cone, union, cut, intersect. Writes
  models/<name>.step, binary .stl and .json (steps plus volume and bounding box per body).

## 0.11.0 — 2026-10-13

- Chat replies and .md files render as Markdown with LaTeX math ($…$, $$…$$, \(…\), \[…\]).
- Stop button ends a running turn (POST /api/projects/<name>/stop); the claude process is killed.
- Thinking level picker for Claude (passes --effort low…max to Claude Code).
- Restyled chat: bubbles, compact tool rows with the file they touch, Enter to send.

## 0.10.1 — 2026-10-13

- Assistant replies in the chat are no longer cut off.
- Model picker is a dropdown listing every model for the provider, plus a custom model option.
- Projects list shows only Forge projects (folders with a git repository or saved chat).
- Added AI-facing docs: AGENTS.md entry point, docs/ARCHITECTURE.md, docs/ROADMAP.md.

## 0.10.0 — 2026-10-12

- Backend rewritten in Rust, running in-process in the desktop app.
- Claude runs through the Claude Code CLI with Forge tools over MCP.
- Node is only needed for the UI build and `pcb_build`.
- Anthropic SDK removed.

## 0.9.1 — 2026-10-12

- New layout modelled on FreeCAD, Blender and KiCad: project and file tree on the left, the selected file in a full-size viewport in the middle, the assistant on the right, and a status bar with the project, mouse hints and the tools list.
- Files show as a folder tree with type colours (models, drawings, boards, code), and the open file is highlighted.
- Neutral grey theme with blue selection; models render in a CAD grey on a gradient viewport, and boards on KiCad's dark canvas.
- The workspaces are now Viewer and Design; picking a file switches to the Viewer.
- The 3D viewer follows the window size.

## 0.9.0 — 2026-10-12

- Model picker in the top bar: switch between Claude, OpenAI, OpenRouter, DeepSeek, Qwen, GLM and OpenCode Zen while Forge runs, with each provider's model list.
- Subscription coding plans work through their API key and endpoint; `FORGE_<ID>_BASE_URL` points a provider at the plan's endpoint.
- `FORGE_PROVIDER` picks the provider Forge starts with.

## 0.8.0 — 2026-10-12

- `rf_antenna_sim`: simulates a board's printed antenna with the openEMS field solver and reports resonance, -10 dB bandwidth, input impedance, efficiency, gain and radiation pattern, with S11 and pattern plots and a Touchstone file; checked against a half-wave dipole.
- `rf_antenna_sim` warns when ground copper under or beside the antenna will detune it, and suggests the arm length change and the matching network to use.
- `scripts/setup-fedora.sh --with-rf` (or `--only rf`) builds openEMS from source.
- `pcb_build` no longer reports a printed antenna's open arm as a disconnected trace.
- Antenna guidance corrected: inverted-F ground short, monopole and folded-dipole feeds, and a keepout that clears the arm by 5 mm.
- `rf_calc` notes that its grounded coplanar waveguide ignores copper thickness.

## 0.7.0 — 2026-10-12

- `rf_calc` line: impedance or trace width for microstrip, grounded coplanar waveguide, stripline and differential pairs, on JLCPCB stackups or any height and permittivity, with the wavelength on the line.
- `rf_calc` match: L matching networks from an antenna's impedance to 50 ohm, with E24 part values and the return loss they achieve.
- `rf_calc` link: link budget and range for free space, outdoor and indoor paths.
- `pcb_build` documents antennas (2.4 GHz monopole, inverted-F, folded dipole), keepouts, copper pours and differential pairs, and the agent designs RF boards with them.
- Copper pours are generated again (the bundled engine now finds its geometry library), and engine steps that fail are reported as errors instead of being skipped silently.
- `pcb_build` lists the board's copper pours and keepout areas.

## 0.6.0 — 2026-10-12

- Built-in PCB engine (tscircuit, `npm run pcb-engine`): `pcb_build` turns a board written as code into an autorouted PCB, schematic, 3D model, Gerbers, drills, pick-and-place and a JLCPCB BOM, with no KiCad needed.
- Board code runs sandboxed: it can only read and write the project, cannot start programs, does not see the API key and stays offline unless part lookup is asked for.
- `design_assemble` puts boards and enclosure parts in one 3D assembly and checks that every component clears the enclosure, reporting collisions and tight spots.
- Design view, like Fusion 360's electronics workspace: the PCB layout or schematic beside the 3D assembly, with cross-selection, collision highlights and a see-through case.
- The agent designs boards and their enclosures together and fixes collisions before calling a design done.
- The Fedora setup script and the desktop app build and ship the PCB engine.

## 0.5.3 — 2026-10-12

- The enlarged image view fits the window instead of overflowing it.

## 0.5.2 — 2026-10-12

- Rebuilding a FreeCAD part no longer leaves a timestamped .FCBak backup in the project.

## 0.5.1 — 2026-10-12

- Hole detection no longer reports the rounded inside corners of a pocket as holes, or a small edge chamfer as a countersink.
- Counterbore depth is measured from the surface, so a chamfered mouth does not shorten it.
- Blind 3.6 mm holes are identified as M2.5 insert holes, not loose M3 clearance.
- Drawings place hole tags clear of other holes, tags and the part outline, and size hole-table columns to their text.

## 0.5.0 — 2026-10-12

- `forge` helper library for FreeCAD scripts: ISO clearance, counterbore, countersink, tapped and heat-set insert holes, rounded boxes, enclosures, lids, standoffs, edge fillets and chamfers, loading parts for assemblies.
- `freecad_inspect`: mass in a chosen material, every hole with the screw it fits, 3D-print overhangs, and interference or clearance between parts.
- `freecad_drawing`: technical drawings with front/top/side and isometric views, hidden lines, dimensions, hole table and title block, in first or third angle projection, as SVG, PDF and DXF.
- PartDesign bodies and constrained Sketcher sketches export as the part.
- "Open in FreeCAD / Blender / KiCad" button on the file preview; a Flatpak gets only the project folder.
- The preview shows SVG drawings, enlarges images on click, and shows the STL beside a STEP or FCStd file.
- setup-fedora.sh and the RPM add librsvg2-tools for PDF drawings.

## 0.4.0 — 2026-10-12

- Desktop app (Tauri): Forge in its own window, with a private server on a free port that stops when the window closes.
- RPM package for Fedora (`npm run desktop:build`), requiring Node.js 22.18+, git and WebKitGTK, and recommending the CAD, PCB and chip tools.
- `scripts/setup-fedora.sh --desktop` installs the build packages, builds the RPM and installs it.
- The server exits when its stdin closes if FORGE_EXIT_WITH_STDIN=1, so it never outlives the desktop app.
- Fixed `setup-fedora.sh --help` printing a line of the script.

## 0.3.0 — 2026-10-12

- Fedora first: scripts/setup-fedora.sh installs dnf packages, FreeCAD from Flathub, builds Forge, and adds a forge command, systemd user service and desktop launcher.
- Blender and FreeCAD also run from Flatpak installs, sandboxed to the project folder.
- The capabilities panel shows the install command for this system: dnf or Flathub on Fedora, apt on Debian/Ubuntu, Homebrew on macOS.
- Settings and the API key load from ~/.config/forge/env.

## 0.2.0 — 2026-10-12

- FreeCAD tool for engineering parts: runs a script headless, exports STEP, STL and FCStd, reports validity, volume and size.
- Blender tool for visual 3D: runs a bpy script headless, exports GLB/STL/OBJ/.blend, renders a PNG with an automatic camera and lights.
- The agent is told when to use FreeCAD, Blender or the quick primitive builder.
- Viewer shows GLB models with their materials and rendered images; tools may accept several program names or a path from FORGE_BLENDER / FORGE_FREECAD.

## 0.1.0 — 2026-10-12

- Agent loop with Claude Opus 5.5: streamed replies, tool input validation, append-only history, refusal fallback.
- Offline demo model, so the full pipeline runs without an API key.
- Projects as git repositories under ~/Forge with saved conversations.
- Tools: workspace files, git, opt-in commands, 3D parts (STL/OpenSCAD), OpenSCAD render, KiCad DRC and fab export, Verilog simulation and Yosys synthesis.
- Web UI: projects, chat with tool timeline, file preview, 3D STL viewer, capabilities panel.
