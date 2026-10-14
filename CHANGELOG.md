# Changelog

## 0.24.0 — 2026-10-14

- Always export a metric IPC-D-356A test netlist and an A4 fabrication SVG with drill symbols/counts, stackup, dimensions and manufacturing notes.
- Add opt-in stencil paste reduction and 60% windowpane openings for large SMD pads; keep copper and mask geometry unchanged.
- Cut true stadium slots with exact circular ends in STEP and GLB instead of bounding cylinders.
- Add rectangular panels with stepped Gerbers/drills, routed tab bridges and mouse-bites or full-panel V-score lines, x/y/both rails, fiducials and tooling holes.
- Export panel pick-and-place with copy-suffixed designators; retain per-board BOM and legacy artifact hashes. Slot-containing STEP/GLB files change to correct their geometry.

## 0.23.0 — 2026-10-14

- Beautify routed copper by default: normalize overlaps, pull octilinear paths tight, chamfer clear corners, remove redundant vias and centre SMD entries.
- Preserve connectivity, clearance, differential-pair copper and reference planes before length/skew tuning and teardrops.
- Add `rules.beautify: false` for the previous raw routing output and report segment, via and corner changes.

## 0.22.0 — 2026-10-14
- Add opt-in differential-pair routing with fixed coupled copper, paired layer changes, ordinary-net fallback and skew tuning.
- Match length groups with clearance-checked meanders and report saved-copper length/skew faults in DRC.
- Solve single-ended and differential microstrip widths from dielectric properties and impedance targets.
- Add span-aware blind/buried vias, split drill files, layer-filtered copper/mask/JSON and partial-depth 3D drills.
- Add clearance-checked teardrops to saved copper, SVG, Gerbers, pours and DRC. Preserve default 0.21.0 artifact bytes.

## 0.21.0 — 2026-10-14

- The autorouter finishes 200-part boards: a generated 200-part, 330-net board routes 330/330 and
  passes DRC on 4 layers (644 s, 2.6 GB peak) and on 2 layers (33 min).
- Inner layers with a pour are planes: no signal tracks, and plane-net SMD pads drop a via into the
  plane instead of growing trace trees. Via thermals always touch the via annulus.
- Routing grid limit raised from 8 to 65 million cells with bitset masks and sparse storage; A* searches
  a window around each net that doubles on failure; obstacle masks are prepared on all cores with
  thread-count-independent results.
- DRC checks copper through a 4 mm spatial index (1 s on the 200-part board).
- Fix: Gerber pours with holes now clear the holes (separate clear-polarity regions).

## 0.20.0 — 2026-10-14

- Schematics draw real wires: grid-snapped symbols, orthogonal wires that avoid bodies and other nets,
  junction dots, ground/supply symbols for power nets, net labels for nets over 8 pins, NC marks.
- `pcb_drc` lists electrical rule warnings (single-pin nets, unconnected or shorted two-pin parts,
  unused parts) after the DRC faults; they do not count as DRC errors.
- SMD pads have rounded corners (25 % of the short side, at most 0.25 mm), flashed with KiCad's
  `RoundRect` Gerber macro on copper, mask and paste.
- Footprints from the KiCad library: `to220`, `to92`, `sot23_6`, `sot89`, `sod323`, `sma`, `smb`, `smc`,
  `msop8`, `msop10`, `microsd`.

## 0.19.0 — 2026-10-14

- Autorouter for complex boards: multi-pin nets grow from the whole routed tree, blockers are ripped
  up locally, failed nets keep negotiating, PathFinder runs up to 200 passes with deterministic
  restarts, and big boards negotiate too (masks computed per net past 128 MiB). 30-part stress board:
  35/40 nets (the five unrouted nets start on enclosed QFN exposed pads), 40/40 with filled vias.
- `rules.via_in_pad` (default false) lets a via sit in an SMD pad of its own net. DRC accepts such
  vias and notes that filled/capped vias-in-pad are required.

## 0.18.0 — 2026-10-13

- Connector footprints: `usb_c_16` (HRO TYPE-C-31-M-12), `usb_micro_b` (Molex 105017-0001),
  `jst_ph_2` to `jst_ph_16` and `barrel_jack` (CUI PJ-102AH), checked against the KiCad libraries.
- Pads can be slotted or non-plated: Excellon writes routed slots (`G85`) and NPTH holes go to the
  NPTH drill file. Oval pads flash as obround Gerber apertures on the 90° grid and stay regions otherwise.
- The autorouter adds PathFinder negotiated congestion when sequential A* leaves nets unrouted.
  Stress boards route 15/15, 19/25 and 29/40 nets (was 15, 13 and 13).

## 0.17.0 — 2026-10-13

- Forge's own PCB engine (stage 3): `pcb_build` reads `pcb/<name>.pcb.json` and generates IPC-7351
  footprints, places, autoroutes (2 or 4 layers), pours copper with thermal reliefs, draws printed
  antennas and writes Gerber X2, Excellon, pick-and-place, BOM, SVG views, GLB and STEP. `pcb_drc`
  runs natively. `rf_calc` adds track width, via, fusing, spacing and E-series calculations.
  Neither Node nor KiCad is needed for circuit boards.

## 0.16.5 — 2026-10-13

- Retired the Node PCB engine and `.tsx`/`.jsx` board format. `pcb_build` is always available
  and accepts only `.pcb.json`; builds, DRC and manufacturing exports run natively.
- Removed engine-only Node settings, isolated execution, part lookup, tests and bundling.
  Fedora setup keeps Node/npm only for the UI build; desktop packages no longer recommend Node.
- Updated board UI, RF-reader wording and documentation. Kept tscircuit's MIT attribution
  and license for the ported antenna geometry. No new dependencies.

## 0.16.4 — 2026-10-13

- Native `pcb_drc` checks saved circuit copper, net-class clearance and track widths, drills,
  annular rings, board edges, connectivity, courtyards and vias in SMD pads. Reports are sorted
  and saved to `fab/<name>/<name>-drc.txt`; no KiCad program is needed.
- Removed `pcb_export_fab`: native `pcb_build` already writes every manufacturing file.
- Added IPC-2221 track width and via estimates, Onderdonk fusing current, IPC-2221B conductor
  spacing and E12/E24/E96 nearest-value and unloaded-divider calculations to `rf_calc`.
- Vias now fan out from SMD pads. Short exits remain available during routing, and rip-up
  passes retain learned net priority. The stress board still routes 26/40 nets with no clearance
  violations; the 555 board routes all nets and passes DRC. Placement reserves through-hole
  courtyards on both sides, preventing bottom pads from overlapping header copper. No new dependencies.

## 0.16.3 — 2026-10-13

- Native `.pcb.json` builds now write PCB and schematic SVGs, GLB and STEP board assemblies,
  every Gerber X2 layer, separate plated/non-plated drills, pick-and-place and BOM CSVs.
  Four-layer builds include both inner copper layers. CAD export shares Forge's kernel lock
  inside the blocking worker; repeated builds produce identical artifacts.
- The PCB tool description and agent prompt now use the native JSON format. Native JSON execution
  needs no Node; legacy `.tsx`/`.jsx` builds keep their existing Node path.
- Build reports count routed pin-to-pin connections rather than route segments and include
  the error-summary prefix. Output tests now run, including RF-reader checks for every antenna
  shape and geometry, drill, coordinate-frame and determinism checks. No new dependencies.

## 0.16.2 — 2026-10-13

- Native `.pcb.json` builds now route two- and four-layer boards, respect holes and keepouts,
  and fill copper pours with clearance, thermal reliefs and hole contours. Routing runs off the
  async runtime and produces deterministic results.
- Ported all five printed antenna geometries and feed-opening keepouts from tscircuit under MIT.
  Native builds write circuit JSON, board boxes, bottom copper Gerber and the build report;
  the remaining outputs follow in step 4. Existing `.tsx` builds still use Node.
- Added `i_overlay` 4.5 for planar copper booleans and native contours. It avoids Cadrum's
  artificial-thickness 3D workaround, which exceeded one second in the PCB spike.

## 0.16.1 — 2026-10-13

- `pcb_build` accepts native `.pcb.json` inputs for strict validation, footprint generation,
  pin/net checks and deterministic placement. Native files arrive in a later stage; `.tsx` builds
  continue using the existing engine.
- Native PCB helpers expose pads, courtyards, package bodies, netlists and placements for the next
  routing and output steps. The footprint and spec reference tests now run offline.

## 0.16.0 — 2026-10-13

- Forge no longer needs FreeCAD. `freecad_build`, `freecad_inspect` and `freecad_drawing` are removed;
  Forge's own CAD kernel covers everything they did.
- `part_build` adds loft (ruled or smooth), sweep along line and arc paths, shell with chosen open
  sides, mirror, linear and polar patterns, ISO metric threaded rods (M2–M12), threaded holes
  (`kind:"thread"`) and sheet metal with bends; sheet bodies report their flat length.
- `part_inspect` reports centre of mass, takes an overhang angle, and with `against` checks every pair
  of parts for overlap volume and clearance.
- New `part_drawing`: front, top, right and iso views with hidden lines, first or third angle, automatic
  sheet (A4–A2) and scale, overall dimensions, hole tags and table, title block, and an optional
  top-view DXF.
- The Flatpak test and setup script now cover Blender only.

## 0.15.1 — 2026-10-13

- The provider, model and thinking-level pickers moved from the menu bar into the chat box, beside Send.
- Every tool schema is portable across providers: each field is typed, `part_build` lists its ops
  and describes `size`, `edges`, `points` and `outline`, and unknown fields are rejected
  (`<path>.<key> is not a known field`). A test enforces the rules.
- Clearer errors for models: unparsable arguments return `Arguments were not valid JSON: …`, and an
  unknown tool lists the available tools. Names such as `functions.part-build` or
  `mcp__forge__part_build` now resolve to the real tool.
- The system prompt mentions only tools that are available, so models stop calling missing FreeCAD,
  Blender or KiCad tools.

## 0.15.0 — 2026-10-13

- Removed `cad_model`; `part_build` replaces it. The offline demo now builds its enclosure with
  `part_build` (STEP, STL and JSON).
- Fixed a crash: OpenCASCADE is not thread-safe, so parts now build and inspect one at a time.
  Parallel builds segfaulted about once in 20 test runs.
- The in-app AI now uses `part_build`/`part_inspect` first and keeps FreeCAD for loft, sweep, shell,
  threads, sheet metal, interference checks and drawings.

## 0.14.1 — 2026-10-13

- `part_build` and `part_inspect` descriptions now list every op with its defaults, the material names
  and a worked enclosure example (covered by a test).

## 0.14.0 — 2026-10-13

- `part_build` adds standard clearance, tap, counterbore, countersink and insert holes, point patterns,
  rounded enclosures, matching lids and standoffs. Engineering dimensions and material densities
  match the existing CAD helpers.
- `part_inspect` reports material mass, STEP hole features and 45-degree printing overhangs above
  the bed. STL inspection reports mass and overhangs while keeping holes marked as unknown.

## 0.13.0 — 2026-10-13

- `part_build` adds line/arc/circle extrusion, Z-axis revolution, fillets and chamfers selected by
  all edges, vertical edges or height, and placement with X/Y/Z rotations followed by translation; `rotate` and `move` default to zero.
- New native `part_inspect` reads STEP/STP and binary or ASCII STL, reporting real size and volume.
  Hole detection, mass and overhang checks remain scheduled for the next CAD kernel step.

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
