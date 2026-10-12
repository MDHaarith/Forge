# Forge

An AI workshop: one conversation that drives the tools for software, 3D/mechanical parts,
printed circuit boards and chips, and records every step in git.

Forge does not reimplement CAD or EDA. It is an orchestrator. Claude plans the work and calls
**tools**; each tool either does the job itself (the built-in mesh kernel) or wraps a proven
open-source program (OpenSCAD, KiCad, Icarus Verilog, Yosys). Every project is a git
repository, and designs are kept as text so they can be diffed, reviewed and rebuilt.

## Run it

Requires Node.js 22.18 or newer and git.

```sh
npm install
export ANTHROPIC_API_KEY=...   # optional: without it Forge runs an offline demo
npm start                      # builds the UI and serves http://127.0.0.1:4317
```

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
| `PORT` | Server port (default 4317). |

## Capabilities

The right-hand panel shows which tools are live. A tool that wraps a program stays hidden
from the agent until that program is on `PATH`, so installing one switches it on.

| Domain | Tools | Needs |
|---|---|---|
| Workspace | `list_files`, `read_file`, `write_file` | – |
| Version control | `git_status`, `git_commit`, `git_log` | git |
| Code | `run_command` | `FORGE_ALLOW_COMMANDS=1` |
| 3D / mechanical | `cad_model` (STL + OpenSCAD + JSON from primitives) | – |
| | `openscad_render` | [OpenSCAD](https://openscad.org) |
| PCB | `pcb_drc`, `pcb_export_fab` (Gerbers + drill for JLCPCB/PCBWay) | [KiCad 8+](https://www.kicad.org) `kicad-cli` |
| Chip | `chip_simulate` | Icarus Verilog |
| | `chip_synthesize` | Yosys |

## Layout

```
server/
  index.ts        HTTP API (localhost only) and static UI
  agent.ts        agent loop: model ⇄ tools, append-only history, input validation
  model.ts        Claude (streaming, adaptive thinking, refusal fallback) and the offline demo model
  projects.ts     project folders, git init, saved conversations (.state/)
  cad/mesh.ts     mesh kernel: box/cylinder/sphere → STL and OpenSCAD
  tools/          one file per domain; registry.ts lists them all
web/              UI: projects, chat with tool timeline, file preview, three.js STL viewer
tests/            node:test suites (no network, no API key needed)
```

## Adding a capability

Write a `ForgeTool` (see `server/tools/types.ts`) with a name, description, JSON schema and
`run` function; set `requires` if it wraps a program; append it in `server/tools/registry.ts`.
The agent, the capabilities panel and input validation pick it up automatically.

## Roadmap

1. Desktop shell: wrap the server and UI in Tauri.
2. Approvals: confirm before tools that write outside `models/`, run commands, or export for manufacture.
3. Real CAD kernel: CadQuery/OpenCascade for booleans, fillets and STEP export.
4. PCB authoring: generate KiCad schematics and boards from a netlist the agent writes; render previews.
5. Chip flow: OpenLane on the SkyWater 130 PDK through to GDS; Tiny Tapeout submission template.
6. Cross-domain links: enclosure sized from the board outline, firmware pin map from the schematic.
7. MCP: expose Forge tools as an MCP server and load third-party MCP servers as capabilities.
