# Forge

An AI workshop: one conversation that drives the tools for software, 3D/mechanical parts,
printed circuit boards and chips, and records every step in git.

Forge does not reimplement CAD or EDA. It is an orchestrator. Claude plans the work and calls
**tools**; each tool either does the job itself (the built-in mesh kernel) or wraps a proven
open-source program (OpenSCAD, KiCad, Icarus Verilog, Yosys). Every project is a git
repository, and designs are kept as text so they can be diffed, reviewed and rebuilt.

## Install on Fedora

Fedora is Forge's primary platform (Fedora Workstation 42+; relatives such as Nobara work too).

```sh
git clone -b forge https://github.com/MDHaarith/GitGUI.git forge && cd forge
scripts/setup-fedora.sh            # or: --only "3d chip", --dry-run to preview
```

The script:

- installs Node.js, git, Blender, OpenSCAD, rsvg-convert (PDF drawings), KiCad, Icarus Verilog and Yosys with `dnf`
  (KiCad is a large download; leave it out with `--only "3d chip"`);
- installs **FreeCAD from Flathub** for your user, because Fedora's repositories have no FreeCAD
  package. Forge runs it inside its Flatpak sandbox and gives it access to the project folder only;
- builds Forge and adds a `forge` command, an app-menu launcher, and a systemd user service;
- creates `~/.config/forge/env` (private, mode 600) for your API key and settings.

Then put your key in `~/.config/forge/env` and start Forge from the app menu or with `forge`.

| Command | Does |
|---|---|
| `forge` | start Forge in the background and open it in the browser |
| `forge stop` / `forge restart` | stop or restart the service |
| `forge logs` | follow the log (`journalctl --user -u forge`) |
| `forge autostart on` | start Forge when you log in |

Programs installed later (with `dnf` or Flatpak) are picked up after `forge restart`, and the
capabilities panel shows the exact install command for anything missing.

### Desktop app

```sh
scripts/setup-fedora.sh --desktop    # adds the Rust and WebKitGTK build packages, builds and installs Forge-<version>.rpm
```

The desktop app (Tauri, `forge-desktop`, "Forge" in the app menu) opens Forge in its own window
instead of the browser. It starts a private Forge server on a free local port, so it never clashes
with the `forge` service, and stops that server when the window closes. It reads the same
`~/.config/forge/env`. The RPM requires `nodejs(engine) >= 22.18`, git and WebKitGTK, and
recommends Blender, OpenSCAD, KiCad, Icarus Verilog and Yosys.

To build the RPM yourself: `npm ci && npm run desktop:build` (output in
`desktop/target/release/bundle/rpm/`). `npm run desktop` runs a development build from the repository.

## Run it anywhere else

Requires Node.js 22.18 or newer and git.

```sh
npm install
export ANTHROPIC_API_KEY=...   # optional: without it Forge runs an offline demo
npm start                      # builds the UI and serves http://127.0.0.1:4317
```

Settings can also go in `~/.config/forge/env` (or the file named by `FORGE_ENV_FILE`); variables
already set in the environment take precedence.

Projects live in `~/Forge/<name>` (override with `FORGE_HOME`). Try a project with
"Design a 60 × 40 mm enclosure with M3 standoffs", then click the STL to orbit it.

For development, `npm run dev` serves the UI with hot reload on Vite's port and restarts
the server on changes. `npm test` runs the test suite, `npm run typecheck` the type check.

| Variable | Effect |
|---|---|
| `ANTHROPIC_API_KEY` | Use Claude. An `ant auth login` profile works too. |
| `FORGE_MODEL` | Model ID (default `claude-opus-5-5`). |
| `FORGE_OFFLINE=1` | Force the offline demo even with credentials. |
| `FORGE_ALLOW_COMMANDS=1` | Let the agent run programs in the project folder (`run_command`). Off by default. |
| `FORGE_HOME` | Where projects are stored. |
| `FORGE_BLENDER`, `FORGE_FREECAD` | Path to `blender` / `freecadcmd` when they are not on `PATH` (e.g. an AppImage). A native install is preferred over a Flatpak. |
| `FORGE_NO_FLATPAK=1` | Ignore Flatpak installs of Blender and FreeCAD. |
| `PORT` | Server port (default 4317). |

## Capabilities

The right-hand panel shows which tools are live. A tool that wraps a program stays hidden
from the agent until that program is on `PATH`, so installing one switches it on.

| Domain | Tools | Needs |
|---|---|---|
| Workspace | `list_files`, `read_file`, `write_file` | – |
| Version control | `git_status`, `git_commit`, `git_log` | git |
| Code | `run_command` | `FORGE_ALLOW_COMMANDS=1` |
| 3D: engineering | `freecad_build`: exact-dimension parts (Part or PartDesign/Sketcher, ISO holes from the `forge` helpers) → STEP, STL, FCStd | [FreeCAD](https://www.freecad.org) 0.21+ (`freecadcmd`), native or Flatpak `org.freecad.FreeCAD` |
| | `freecad_inspect`: mass in a material, holes and the screws they fit, print overhangs, interference and clearance between parts | FreeCAD |
| | `freecad_drawing`: technical drawing (views, hidden lines, dimensions, hole table, title block) → SVG, PDF, DXF | FreeCAD; `rsvg-convert` for the PDF |
| 3D: visual | `blender_build`: models, materials, scenes → GLB/STL/OBJ/.blend, optional Cycles render to PNG | [Blender](https://www.blender.org) 4.0+, native or Flatpak `org.blender.Blender` |
| 3D: quick | `cad_model` (STL + OpenSCAD + JSON from primitives) | – |
| | `openscad_render` | [OpenSCAD](https://openscad.org) |
| PCB | `pcb_drc`, `pcb_export_fab` (Gerbers + drill for JLCPCB/PCBWay) | [KiCad 8+](https://www.kicad.org) `kicad-cli` |
| Chip | `chip_simulate` | Icarus Verilog |
| | `chip_synthesize` | Yosys |

## Blender and FreeCAD

The agent picks the 3D tool by purpose. **FreeCAD** is for engineering parts that need exact
dimensions, holes, fits and fillets, and a STEP file a machine shop or other CAD program can
open. **Blender** is for visual work: organic shapes, materials, scenes and rendered images.

Both work the same way. The agent writes a Python script into the project (`cad/*.py` for
FreeCAD, `art/*.py` for Blender), Forge runs it in the real program with no window
(`server/tools/runners/`), and the exported files appear in the project and the viewer. The
script stays in git, so a design is reproducible and can be edited and re-run.

- FreeCAD scripts get `App`, `Part`, `Vector`, a new document `doc` and the `forge` helpers, and
  set `result = <shape>` or add objects to `doc` (PartDesign bodies and constrained sketches
  work too). Forge reports whether the solid is valid, its volume and its bounding box.
- The `forge` helpers (`server/tools/runners/forge_cad.py`) know ISO metric hardware:
  `forge.hole(part, x, y, "M3", kind="counterbore")` cuts a clearance hole with the right
  counterbore for an ISO 4762 screw; `kind` can also be `"countersink"`, `"tap"` (tap drill)
  or `"insert"` (heat-set insert). There are also `rounded_box`, `enclosure`, `lid`,
  `standoffs`, `fillet`/`chamfer` by edge selection, and `load` for STEP/FCStd parts to assemble.
- `freecad_inspect` measures the result: mass in a chosen material, every hole with the screw
  it fits, overhang area for 3D printing, and interference or clearance between the parts of
  an assembly. `freecad_drawing` makes a manufacturing drawing in first or third angle
  projection on an A4–A2 sheet, with a hole table and title block, as SVG and PDF, plus the
  top outline as DXF for laser cutting.
- **Open in FreeCAD / Blender / KiCad**: the preview has a button that opens the file in the
  installed desktop program (a Flatpak gets only the project folder), to refine it by hand.
- Blender scripts start from an empty scene. With `render: true`, Forge adds a camera framed on
  the subject and lights if the script did not, and renders with Cycles on the CPU so it works
  on machines without a GPU.

Some distribution packages of Blender lack numpy (needed for GLB export) or the
denoiser; Forge renders without denoising when that library is missing. The official
downloads include both.

## Layout

```
server/
  index.ts        HTTP API (localhost only) and static UI
  env.ts          loads ~/.config/forge/env before anything reads settings
  platform.ts     distro detection and per-distro install commands (dnf first)
  agent.ts        agent loop: model ⇄ tools, append-only history, input validation
  model.ts        Claude (streaming, adaptive thinking, refusal fallback) and the offline demo model
  projects.ts     project folders, git init, saved conversations (.state/)
  cad/mesh.ts     mesh kernel: box/cylinder/sphere → STL and OpenSCAD
  tools/script3d.ts         Blender and FreeCAD build tools
  tools/freecad.ts          FreeCAD inspection and technical drawings
  open.ts                   "Open in FreeCAD/Blender/KiCad" for project files
  tools/launch.ts           runs a program natively or as a Flatpak with only the folders it needs
  tools/runners/*.py        scripts that run inside Blender / FreeCAD; forge_cad.py is the helper library
  tools/          one file per domain; registry.ts lists them all
desktop/          Tauri desktop app (Rust): starts the server, shows the UI, builds the RPM
web/              UI: projects, chat with tool timeline, file preview, three.js STL/GLB viewer, render images
scripts/setup-fedora.sh   Fedora installer: dnf packages, FreeCAD Flatpak, build, forge command, service, launcher
scripts/stage-app.sh      gathers the server, built UI and runtime packages the desktop app bundles
tests/            node:test suites (no network, no API key needed)
```

## Adding a capability

Write a `ForgeTool` (see `server/tools/types.ts`) with a name, description, JSON schema and
`run` function; set `requires` if it wraps a program; append it in `server/tools/registry.ts`.
The agent, the capabilities panel and input validation pick it up automatically.

## Roadmap

1. Desktop shell as a Flatpak (the RPM is done). Needs host access through `flatpak-spawn --host`
   so the sandboxed app can still run Blender, KiCad and the other tools installed on the system.
2. Approvals: confirm before tools that write outside `models/`, run commands, or export for manufacture.
3. FreeCAD assemblies with joints (parts can already be loaded, placed and checked for
   interference), GD&T tolerances on drawings, and CAM toolpaths.
4. PCB authoring: generate KiCad schematics and boards from a netlist the agent writes; render previews.
5. Chip flow: OpenLane on the SkyWater 130 PDK through to GDS; Tiny Tapeout submission template.
6. Cross-domain links: enclosure sized from the board outline, firmware pin map from the schematic.
7. MCP: expose Forge tools as an MCP server and load third-party MCP servers as capabilities.
