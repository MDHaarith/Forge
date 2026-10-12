# Changelog

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
