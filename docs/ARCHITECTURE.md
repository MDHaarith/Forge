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
    board.rs pcb.rs design.rs fit.rs         PCB engine, DRC, fab export, board-in-enclosure fit
    pcb/spec.rs                             strict native .pcb.json parser, pin aliases, netlist and rule overrides
    pcb/footprint.rs                        pure nominal footprint generator, pads, courtyard, silk and body geometry
    pcb/place.rs                            deterministic placement, rotated courtyards, polygon edges and hole checks
    rf.rs rfmath.rs                          rf_calc, rf_antenna_sim (openEMS)
    chip.rs                                  Verilog simulate/synthesize
server/runners/*.py        Blender and openEMS scripts; forge_art.py = visual modelling helpers
server/tests/              integration tests (common/ has TempDir, fake Claude, tool helpers)
desktop/src/main.rs        Tauri app: binds a free port, runs forge_server::serve in-process
engines/pcb/               tscircuit PCB engine (JavaScript), bundled to dist/pcb-engine.mjs, run by Node
web/                       UI (TypeScript + Vite + three.js): main.ts app, design.ts PCB/3D view, viewer.ts,
                           markdown.ts (Markdown + KaTeX, sanitized with DOMPurify)
scripts/                   setup-fedora.sh installer, stage-app.sh bundles files for the desktop app
docs/                      this map, ROADMAP.md, reference/ (engineering CAD, KiCad and Blender feature checklists)
```

`pcb.rs` remains the PCB module root and re-exports the native pure helpers. `pcb_build` validates
and places `.pcb.json` inputs in `spawn_blocking`, returning no files until the output step is built.
The existing `.tsx`/`.jsx` path still runs the Node engine and writes its usual outputs.

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
