# Forge

An AI workshop: one conversation that drives the tools for software, 3D/mechanical parts,
printed circuit boards and chips, and records every step in git.

Claude plans the work and calls **tools**; each tool either does the job itself
(the built-in OpenCASCADE CAD kernel) or wraps a proven open-source program
(Blender, KiCad, Icarus Verilog, Yosys); circuit boards are built
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
`~/.config/forge/env`. The RPM requires git and WebKitGTK, and recommends Blender, OpenSCAD,
KiCad, Icarus Verilog and Yosys. The window starts without Node.

To build the RPM yourself: `npm ci && npm run desktop:build` (output in
`target/release/bundle/rpm/`). `npm run desktop` runs a development build from the repository;
run `sh scripts/stage-app.sh` once first so Tauri's resource folder exists.

## Run it anywhere else

Requires the Rust toolchain (Rust and Cargo), Node.js 22.18 or newer for the UI build,
and git. PCB builds run natively without Node.

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
| `FORGE_PROVIDER` | Provider to start with: `claude`, `openai`, `openrouter`, `deepseek`, `qwen`, `glm`, `opencode` or `offline`. Default: Claude when its CLI is available, else offline. |
| `FORGE_MODEL` | Model ID for the startup provider (default `claude-opus-5-5`). |
| `FORGE_OFFLINE=1` | Force the offline demo even with credentials. |
| `FORGE_ALLOW_COMMANDS=1` | Let the agent run programs in the project folder (`run_command`). Off by default. |
| `FORGE_HOME` | Where projects are stored. |
| `FORGE_APP_DIR` | Folder holding `dist/` and `server/runners/` for a standalone binary. |
| `FORGE_BLENDER` | Path to `blender` when it is not on `PATH` (e.g. an AppImage). A native install is preferred over a Flatpak. |
| `FORGE_NO_FLATPAK=1` | Ignore Flatpak installs of Blender and KiCad. |
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
| 3D: engineering | `part_build`: exact solids from Forge's own CAD kernel (OpenCASCADE): shapes, extrude, revolve, loft, sweep, shell, mirror, patterns, threads, sheet metal, booleans, fillet, chamfer, ISO holes, enclosure, lid, standoffs → STEP, STL, JSON | – |
| | `part_inspect`: size, volume, centre of mass, mass in a material, holes and the screws they fit, print overhangs, interference and clearance with `against` | – |
| | `part_drawing`: scaled technical views, hidden lines, dimensions, hole table and title block → SVG, top-outline DXF | – |
| 3D: visual | `blender_build`: models, materials, scenes → GLB/STL/OBJ/.blend, optional Cycles render to PNG | [Blender](https://www.blender.org) 4.0+, native or Flatpak `org.blender.Blender` |
| 3D: quick | `openscad_render` | [OpenSCAD](https://openscad.org) |
| PCB | `pcb_build`: native `.pcb.json` board → autorouted PCB, schematic, 3D model, Gerbers, drills, pick-and-place, JLCPCB BOM; `pcb_drc`: saved-copper checks | none |
| | `design_assemble`: boards and enclosure parts in one 3D assembly, with a fit check | – |
| RF | `rf_calc`: impedance and trace width (microstrip, coplanar, stripline, differential) on fab stackups, antenna matching networks, link budget and range | – |
| | `rf_antenna_sim`: full-wave simulation of the board's antenna → S11, bandwidth, impedance, efficiency, gain pattern, Touchstone file | openEMS (`--with-rf`) |
| | `pcb_drc`, `pcb_export_fab` for KiCad projects (Gerbers + drill for JLCPCB/PCBWay) | [KiCad 8+](https://www.kicad.org) `kicad-cli` |
| Chip | `chip_simulate` | Icarus Verilog |
| | `chip_synthesize` | Yosys |

## Exact CAD and Blender

The agent uses **`part_build`** for every exact engineering part: dimensions, holes, fits,
fillets, lofts, sweeps, shells, patterns, threads and sheet metal. Forge's built-in CAD kernel
exports STEP for manufacturing, STL for printing and previews, and JSON metadata.
**Blender** handles visual work: organic shapes, materials, scenes and rendered images.

- `part_inspect` measures size, volume, centre of mass, material mass, holes and print overhangs.
  Use `against` to check interference and clearance between mating parts.
- `part_drawing` writes scaled technical SVG drawings with hidden lines, dimensions, a hole table
  and title block, plus the top outline as DXF. PDF output remains planned.
- The agent writes Blender Python scripts in `art/`. Forge runs them headless through
  `server/runners/`, then shows the exported files. Scripts stay in git for editing and rebuilding.
- Blender scripts get their own `forge` helpers (`server/runners/forge_art.py`): bevel,
  solidify, subdivision, array, mirror, exact booleans, screw/lathe, decimate, remesh,
  shrinkwrap, displace, twist/bend deforms and more as modifiers; face extrude, inset and bisect
  as mesh edits; `apply_all` to bake them; and metal, plastic, rubber, glass, ceramic and
  emissive material presets. Objects the script hides (boolean cutters) are left out of the
  exports.
- **Open in Blender / KiCad**: the preview has a button that opens supported files in the
  installed desktop program (a Flatpak gets only the project folder), to refine them by hand.
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

- **The board** is JSON in `pcb/<name>.pcb.json`. Native `pcb_build` validates connections,
  generates footprints, places parts, autoroutes and pours copper. It writes `pcb/` views and
  models and `fab/<name>/` manufacturing files ready for JLCPCB. Builds stay offline; set each
  part's `lcsc` field for BOM part numbers.
- **The case** is built with `part_build`, sized from the board's size, holes and tallest part.
- **`design_assemble`** places both in `design/<name>.json` and checks every component's box
  against the case's triangles, so the agent can move a connector or raise a lid until it fits.

## Radios and antennas

Boards with Wi-Fi, Bluetooth, Zigbee, LoRa or other radios get an RF workflow:

1. **Trace widths.** `rf_calc` (calc `line`) gives the width for a 50 ohm feed or a 90/100 ohm
   differential pair on JLCPCB's stackups (or any height and permittivity): microstrip with
   frequency dispersion (Hammerstad-Jensen, Kirschning-Jansen), grounded coplanar waveguide
   (conformal mapping), stripline (Wheeler/Cohn) and coupled microstrip.
2. **The antenna.** A part with `kind:"antenna"` draws 2.4 GHz monopoles, inverted-F antennas
   and a folded dipole; `pours` on the other layer provide ground. Add `keepouts` that clear
   the radiating arm by 5 mm or more out to the board edge, leaving ground under the feed pad.
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

## For developers and AI agents

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): how a request flows, every file, the API, how to add a tool.
- [docs/ROADMAP.md](docs/ROADMAP.md): the plan to make Forge run with no external CAD/EDA programs.
- [AGENTS.md](AGENTS.md): rules every change must follow.

## Licence

Forge is MIT licensed (see [LICENSE](LICENSE)). It bundles Open CASCADE Technology under the LGPL 2.1
with the Open CASCADE exception; see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
