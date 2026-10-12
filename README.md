# Forge

An AI workshop: one conversation that drives the tools for software, 3D/mechanical parts,
printed circuit boards and chips, and records every step in git.

Forge does not reimplement CAD or EDA. It is an orchestrator. Claude plans the work and calls
**tools**; each tool either does the job itself (the built-in mesh kernel) or wraps a proven
open-source program (FreeCAD, Blender, KiCad, Icarus Verilog, Yosys); circuit boards are built
by a PCB engine that ships with Forge. Every project is a git
repository, and designs are kept as text so they can be diffed, reviewed and rebuilt.

## Install on Fedora

Fedora is Forge's primary platform (Fedora Workstation 42+; relatives such as Nobara work too).

```sh
git clone -b forge https://github.com/MDHaarith/GitGUI.git forge && cd forge
scripts/setup-fedora.sh            # or: --only "3d chip", --dry-run to preview
```

The script:

- installs Rust, Cargo, Node.js, git, Blender, OpenSCAD, rsvg-convert (PDF drawings), KiCad, Icarus Verilog and Yosys with `dnf`
  (KiCad is a large download; leave it out with `--only "3d chip"`);
- installs **FreeCAD from Flathub** for your user, because Fedora's repositories have no FreeCAD
  package. Forge runs it inside its Flatpak sandbox and gives it access to the project folder only;
- with `--with-rf` (or `--only rf` later), builds the openEMS field solver from source into
  `~/opt/openEMS` for antenna simulation (about 20 minutes);
- builds Forge and its built-in PCB engine, and adds a `forge` command, an app-menu launcher, and a systemd user service;
- creates `~/.config/forge/env` (private, mode 600) for your settings and optional provider keys.

Then sign in with the Claude Code CLI (`claude`), or put another provider's key in
`~/.config/forge/env`, and start Forge from the app menu or with `forge`.

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
with the `forge` service, and runs that Rust server in-process until the app exits. It reads the same
`~/.config/forge/env`. The RPM requires git and WebKitGTK, and recommends Node.js for
`pcb_build`, Blender, OpenSCAD, KiCad, Icarus Verilog and Yosys. The window starts without Node.

To build the RPM yourself: `npm ci && npm run pcb-engine && npm run desktop:build` (output in
`target/release/bundle/rpm/`). The PCB engine is optional; leave out `npm run pcb-engine` if
you do not need `pcb_build`. `npm run desktop` runs a development build from the repository;
run `sh scripts/stage-app.sh` once first so Tauri's resource folder exists.

## Run it anywhere else

Requires the Rust toolchain (Rust and Cargo), Node.js 22.18 or newer for the UI build,
and git. Node is only needed at runtime for `pcb_build`.

```sh
npm install
claude                         # optional: sign in with the Claude Code CLI
npm start                      # builds the UI and serves http://127.0.0.1:4317
```

Settings can also go in `~/.config/forge/env` (or the file named by `FORGE_ENV_FILE`); variables
already set in the environment take precedence.

Cargo runs use the repository's app folder, including release builds. A standalone release
binary uses its own folder; set `FORGE_APP_DIR` when the UI and runners live elsewhere.

Projects live in `~/Forge/<name>` (override with `FORGE_HOME`). Try a project with
"Design a 60 × 40 mm enclosure with M3 standoffs", then click the STL to orbit it.

For development, `npm run dev` starts the Rust server and serves the UI with hot reload
on Vite's port. Restart it after backend changes. `cargo test --workspace` (or `npm test`)
runs the test suite; `npm run typecheck` checks the UI and Vite config.

| Variable | Effect |
|---|---|
| `FORGE_CLAUDE` | Path to the Claude Code CLI; defaults to `claude` on `PATH`. Uses that CLI's login. |
| `FORGE_NODE` | Path to Node for `pcb_build`; defaults to `node` on `PATH`. |
| `FORGE_PROVIDER` | Provider to start with: `claude`, `openai`, `openrouter`, `deepseek`, `qwen`, `glm`, `opencode` or `offline`. Default: Claude when its CLI is available, else offline. |
| `FORGE_MODEL` | Model ID for the startup provider (default `claude-opus-5-5`). |
| `FORGE_OFFLINE=1` | Force the offline demo even with credentials. |
| `FORGE_ALLOW_COMMANDS=1` | Let the agent run programs in the project folder (`run_command`). Off by default. |
| `FORGE_HOME` | Where projects are stored. |
| `FORGE_APP_DIR` | Folder holding `dist/`, `server/runners/` and `engines/pcb/dist/` for a standalone binary. |
| `FORGE_BLENDER`, `FORGE_FREECAD` | Path to `blender` / `freecadcmd` when they are not on `PATH` (e.g. an AppImage). A native install is preferred over a Flatpak. |
| `FORGE_NO_FLATPAK=1` | Ignore Flatpak installs of Blender and FreeCAD. |
| `PORT` | Server port (default 4317). |

## Models and subscriptions

Pick the provider and model from the dropdown in the top bar. Claude uses your installed
**Claude Code CLI** (`claude` on `PATH`, or `FORGE_CLAUDE`) and its existing login, with a
subscription or an API key. Forge needs no `ANTHROPIC_API_KEY` and offers no separate Claude
login. Forge's tools reach Claude Code over MCP, and the next turn resumes its saved session.

The list also shows providers with a key in `~/.config/forge/env`, and the model box suggests
what the provider offers. Keys are only read from that file or the environment, never sent
from the page.

```sh
# ~/.config/forge/env
OPENAI_API_KEY=...         # OpenAI
OPENROUTER_API_KEY=...     # OpenRouter
DEEPSEEK_API_KEY=...       # DeepSeek
DASHSCOPE_API_KEY=...      # Qwen (Alibaba Model Studio)
ZAI_API_KEY=...            # GLM (Z.ai); the default endpoint is the GLM Coding Plan's
OPENCODE_API_KEY=...       # OpenCode Zen
```

Subscription coding plans are an API key on the plan's own endpoint. Override any provider's
endpoint with `FORGE_<ID>_BASE_URL`, for example:

```sh
FORGE_GLM_BASE_URL=https://api.z.ai/api/paas/v4          # pay-as-you-go GLM instead of the Coding Plan
FORGE_QWEN_BASE_URL=https://dashscope.aliyuncs.com/compatible-mode/v1   # mainland China region
```

Check the endpoint your plan's page gives you. Every provider other than Claude goes through its
OpenAI-compatible chat API, without streaming, so a long reply appears when it is complete.

Anthropic does not allow third-party products to offer claude.ai login. Recheck its terms
before distributing Forge.

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
| PCB | `pcb_build`: board from tscircuit code → autorouted PCB, schematic, 3D model, Gerbers, drills, pick-and-place, JLCPCB BOM | Node.js and the built-in engine (`npm run pcb-engine`) |
| | `design_assemble`: boards and enclosure parts in one 3D assembly, with a fit check | – |
| RF | `rf_calc`: impedance and trace width (microstrip, coplanar, stripline, differential) on fab stackups, antenna matching networks, link budget and range | – |
| | `rf_antenna_sim`: full-wave simulation of the board's antenna → S11, bandwidth, impedance, efficiency, gain pattern, Touchstone file | openEMS (`--with-rf`) |
| | `pcb_drc`, `pcb_export_fab` for KiCad projects (Gerbers + drill for JLCPCB/PCBWay) | [KiCad 8+](https://www.kicad.org) `kicad-cli` |
| Chip | `chip_simulate` | Icarus Verilog |
| | `chip_synthesize` | Yosys |

## Blender and FreeCAD

The agent picks the 3D tool by purpose. **FreeCAD** is for engineering parts that need exact
dimensions, holes, fits and fillets, and a STEP file a machine shop or other CAD program can
open. **Blender** is for visual work: organic shapes, materials, scenes and rendered images.

Both work the same way. The agent writes a Python script into the project (`cad/*.py` for
FreeCAD, `art/*.py` for Blender), Forge runs it in the real program with no window
(`server/runners/`), and the exported files appear in the project and the viewer. The
script stays in git, so a design is reproducible and can be edited and re-run.

- FreeCAD scripts get `App`, `Part`, `Vector`, a new document `doc` and the `forge` helpers, and
  set `result = <shape>` or add objects to `doc` (PartDesign bodies and constrained sketches
  work too). Forge reports whether the solid is valid, its volume and its bounding box.
- The `forge` helpers (`server/runners/forge_cad.py`) know ISO metric hardware:
  `forge.hole(part, x, y, "M3", kind="counterbore")` cuts a clearance hole with the right
  counterbore for an ISO 4762 screw; `kind` can also be `"countersink"`, `"tap"` (tap drill)
  or `"insert"` (heat-set insert). There are also `rounded_box`, `enclosure`, `lid`,
  `standoffs`, `fillet`/`chamfer` by edge selection, and `load` for STEP/FCStd parts to assemble.
- They also cover the feature set of a parametric CAD program: profiles with rounded corners and
  arcs; extrude (symmetric, two-sided, tapered, twisted), revolve, loft, sweep and pipe; shell,
  draft, offset, thicken and press-pull; fillets (variable too) and chamfers on edges chosen with
  CadQuery-style selectors such as `forge.edges(part, "|Z")` or `forge.faces(part, ">Z")`;
  linear, polar and path patterns, mirror, scale and split; ribs, slots, coil springs, embossed
  and engraved text; modelled ISO metric threads (`thread`, `threaded_hole`); and sheet-metal
  parts bent from a profile with their flat pattern.
- Blender scripts get their own `forge` helpers (`server/runners/forge_art.py`): bevel,
  solidify, subdivision, array, mirror, exact booleans, screw/lathe, decimate, remesh,
  shrinkwrap, displace, twist/bend deforms and more as modifiers; face extrude, inset and bisect
  as mesh edits; `apply_all` to bake them; and metal, plastic, rubber, glass, ceramic and
  emissive material presets. Objects the script hides (boolean cutters) are left out of the
  exports. [docs/3d-operations.md](docs/3d-operations.md) maps Fusion 360, FreeCAD and Blender
  operations to these helpers and lists what is left out and why.
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

## Boards and enclosures in one view

Like Fusion 360's electronics + mechanical workspace, Forge designs the board and its case
together. The **Design** tab (top bar) shows the selected board's PCB layout (or schematic) beside
the 3D assembly. Clicking a part in either view selects it in the other, collisions are outlined
in red and tight spots in amber, and "See-through case" makes the enclosure transparent.

- **The board** is tscircuit code (React-style `<board>`, `<resistor>`, `<chip>`, `<trace>`)
  in `pcb/<name>.tsx`. `pcb_build` runs it in Forge's PCB engine (`engines/pcb`, bundled into
  one file), which places parts, autoroutes and writes `pcb/` views and models and `fab/<name>/`
  files ready for JLCPCB. The board code runs in a Node permission sandbox: it can read and
  write only the project, start no programs and does not see Forge's settings or API key, and
  it stays offline unless `lookup_parts` is set.
- **The case** is a FreeCAD script, sized from the board's size, holes and tallest part.
- **`design_assemble`** places both in `design/<name>.json` and checks every component's box
  against the case's triangles, so the agent can move a connector or raise a lid until it fits.

The engine has its own `engines/pcb/package.json` (tscircuit needs TypeScript 5, Forge uses 7);
`npm run pcb-engine` installs and bundles it, and the setup script does this for you.
The desktop build includes the engine if it is already built.

## Radios and antennas

Boards with Wi-Fi, Bluetooth, Zigbee, LoRa or other radios get an RF workflow:

1. **Trace widths.** `rf_calc` (calc `line`) gives the width for a 50 ohm feed or a 90/100 ohm
   differential pair on JLCPCB's stackups (or any height and permittivity): microstrip with
   frequency dispersion (Hammerstad-Jensen, Kirschning-Jansen), grounded coplanar waveguide
   (conformal mapping), stripline (Wheeler/Cohn) and coupled microstrip.
2. **The antenna.** tscircuit's `<antenna>` draws 2.4 GHz monopoles, inverted-F antennas and a
   folded dipole; a `<copperpour>` on the other layer is their ground. Their built-in keepout
   clears only about 1 mm, so add a `<keepout>` that clears the radiating arm by 5 mm or more out
   to the board edge, leaving ground under the feed pad.
3. **Simulation.** `rf_antenna_sim` meshes the antenna's copper, the substrate and the ground
   pours for openEMS (an FDTD field solver), excites the feed and reports where it resonates,
   its -10 dB bandwidth, input impedance, efficiency and gain, with S11 and pattern plots and a
   Touchstone `.s1p`. It is checked against a half-wave dipole (resonance, ~70 ohm, ~2.1 dBi),
   and warns when ground copper under or within 2 mm of the arm will detune the antenna.
4. **Matching.** `rf_calc` (calc `match`) turns the simulated impedance into L networks with
   E24 part values for a pi footprint; tune the final values on a VNA.
5. **Range.** `rf_calc` (calc `link`) estimates range from transmit power, antenna gains and the
   receiver's sensitivity for free space, outdoor and indoor paths.

openEMS is not packaged for Fedora; `scripts/setup-fedora.sh --with-rf` builds it into
`~/opt/openEMS` (about 20 minutes). Elsewhere, build it per the
[openEMS install guide](https://docs.openems.de/en/latest/install/requirements.html) and set
`FORGE_OPENEMS_PYTHON` to its venv's `python3`.

## Layout

```
server/src/
  main.rs, http.rs          standalone server, localhost HTTP API and static UI
  lib.rs, env.rs            shared server entrypoint and ~/.config/forge/env settings
  platform.rs              distro detection and per-distro install commands (dnf first)
  agent.rs                 agent loop: model ⇄ tools, append-only history, input validation
  model.rs                 OpenAI-compatible providers and the offline demo model
  claude_code.rs, mcp.rs    Claude Code CLI turns and Forge's HTTP MCP endpoint
  projects.rs              project folders, git init, saved conversations (.state/)
  tools/mesh.rs            mesh kernel: box/cylinder/sphere → STL and OpenSCAD
  tools/script3d.rs         Blender build tools
  tools/freecad.rs          FreeCAD builds, inspection and technical drawings
  open.rs                  "Open in FreeCAD/Blender/KiCad" for project files
  tools/launch.rs           runs a program natively or as a Flatpak with only the folders it needs
  tools/board.rs            pcb_build: runs the PCB engine in a sandboxed Node
  tools/design.rs, fit.rs   design_assemble and the board-in-enclosure fit check
  tools/rf.rs, rfmath.rs    rf_calc (transmission lines, matching, link budget) and rf_antenna_sim
  tools/                   one file per domain; mod.rs lists them all
server/runners/*.py        scripts inside Blender / FreeCAD / openEMS; forge_cad.py is the CAD helper library
server/tests/             Rust tests with scripted models and fake Claude CLIs (no network or API key)
desktop/          Tauri desktop app (Rust): runs the server in-process, shows the UI, builds the RPM
engines/pcb/      the PCB engine: tscircuit, its exporters and runner.mjs, bundled to dist/pcb-engine.mjs
web/              UI: projects, chat with tool timeline, file preview, three.js STL/GLB viewer, render images
web/design.ts     Design view: PCB/schematic beside the 3D assembly, cross-selection, fit highlights
scripts/setup-fedora.sh   Fedora installer: dnf packages, FreeCAD Flatpak, build, forge command, service, launcher
scripts/stage-app.sh      gathers the built UI, Python runners and optional PCB engine the desktop app bundles
```

## Adding a capability

Write a `Tool` (see `server/src/tools/mod.rs`) with a name, description, JSON schema and
`run` function; set `requires` if it wraps a program; append it in `all_tools` there.
The agent, the capabilities panel and input validation pick it up automatically.

## Roadmap

1. Desktop shell as a Flatpak (the RPM is done). Needs host access through `flatpak-spawn --host`
   so the sandboxed app can still run Blender, KiCad and the other tools installed on the system.
2. Approvals: confirm before tools that write outside `models/`, run commands, or export for manufacture.
3. FreeCAD assemblies with joints (parts can already be loaded, placed and checked for
   interference), GD&T tolerances on drawings, and CAM toolpaths.
4. PCB: export the built-in engine's boards to KiCad; manual part moves in the Design view.
5. Chip flow: OpenLane on the SkyWater 130 PDK through to GDS; Tiny Tapeout submission template.
6. Cross-domain links: firmware pin map from the schematic; enclosure generated straight from the board data.
7. MCP: load third-party MCP servers as capabilities (Forge's tools already reach Claude over MCP).
