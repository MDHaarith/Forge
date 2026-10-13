# Plan: Patch 0.15.1, model picker in the chat box and tools any model can call

Written 2026-10-13 for an AI session that has no memory of earlier chats. The finished stage 1 plan
(Forge's own CAD kernel) is in git at commit `ed752b42`. Stage 2 (loft, sweep, shell, patterns,
threads, sheet metal, interference, drawings) comes after this patch.

**If you are the orchestrator (the session that reads this first):**
1. Read this whole file, then `AGENTS.md`, `docs/ARCHITECTURE.md` and `docs/ROADMAP.md`.
2. Find the first unticked step under "Work order".
3. Send that step's brief to the model named on it. Copy the brief exactly and add nothing.
4. Check the result with that step's "Orchestrator check".
5. Tick the box and commit. Then do the next step.

Do one step at a time. Never let two agents edit the checkout at once.

---

## 1. Where things stand

- Forge is at v0.15.0 on `main`. The provider, model and thinking-level selects sit in the top menu
  bar (`web/index.html`, `<form id="model-pick">`), far from the chat box.
- Every provider calls the same tools:
  - **Claude Code CLI** calls them over MCP (`server/src/mcp.rs`).
  - **OpenAI-compatible providers** (openai, openrouter, deepseek, qwen, glm, opencode) call them
    through `server/src/model.rs` `OpenAiCompatible` and the loop in `server/src/agent.rs`.
  - Both paths end in `agent.rs` `execute_tool`, which checks the input with `tools/validate.rs`.
- An audit on 2026-10-13 (offline, FreeCAD hidden: 13 tools, about 15 400 characters of
  descriptions and schemas) found five things that trip models weaker than Claude:
  1. **Untyped `part_build` fields.** `size`, `edges` and `points` have the schema `{}`, and `op` is a
     bare string with no `enum`. Some OpenAI-compatible APIs reject untyped schema nodes, and weak
     models guess the shape.
  2. **Bad JSON gives a wrong error.** When a model sends arguments that are not JSON, `model.rs`
     stores `{"_unparsed": "<text>"}`. The validator then answers "input.x is required", which
     hides the real problem.
  3. **Mangled tool names fail.** Some models call `functions.part_build`, `mcp__forge__part_build`
     or `part-build`. The reply `Unknown or unavailable tool "<name>".` does not list the real names.
  4. **The system prompt names tools that may be missing.** `agent.rs` `system_prompt` always mentions
     `freecad_build`, `freecad_inspect`, `freecad_drawing`, `blender_build`, `rf_antenna_sim` and
     "KiCad tools", even when they are absent. No KiCad tool exists at all. Weak models then call
     the missing tools.
  5. **Misspelled keys pass silently.** A misspelled key (`content` for `contents`, `length` for
     `height`) is ignored, and the tool runs with defaults.

## 2. What the patch delivers

- The provider, model and thinking-level selects live in the chat box, on the row with Send.
- Every tool schema is portable. A test enforces the rules in 3.2.
- Bad JSON, a mangled tool name and an unknown key each get one clear, fixed error that tells the
  model how to fix the call.
- The system prompt names only tools that are available.
- A scripted "weak model" test proves the recovery: bad JSON, then a mangled name, then a correct call.

## 3. The contract (every model follows this exactly)

### 3.1 Exact strings

| Case | Text returned to the model (tool result, `is_error` true) |
|---|---|
| Arguments not JSON (the input is an object whose only key is `_unparsed`) | `Arguments were not valid JSON: <serde_json error>. Send the input as one JSON object.` |
| Tool not found, even after normalising the name | `Unknown or unavailable tool "<name as sent>". Available tools: <names, comma and space separated, in registry order>.` |
| Unknown key, where the schema has `"additionalProperties": false` | `<path>.<key> is not a known field` (inside the existing `INVALID_INPUT` list) |
| No `anyOf` branch matches | `<path> must be <a/an type>, <a/an type> or <a/an type>`, e.g. `input.steps[0].size must be a number, a string or an array` (inside `INVALID_INPUT`) |

- Type words: `a number`, `an integer`, `a string`, `a boolean`, `an array`, `an object`.
- With two branches the message reads `<path> must be a number or a string`.
- Leave every other existing message word for word. In particular, `part_build`'s
  `step <index>: unknown op "<op>"` stays, for direct callers.

### 3.2 Portable schema rules (checked for every tool in `tools::all_tools()`)

1. The root is `"type":"object"` with a `properties` object.
2. Every schema node has `type` or a non-empty `anyOf`. Nodes are the root, each property, `items`
   and each `anyOf` branch, at any depth. No `{}` nodes.
3. No `$ref`, `oneOf`, `allOf`, `not`, `if`, `then` or `else`.
4. Every `type:"array"` node has `items`.
5. Every name in `required` exists in `properties`.
6. Every top-level property has a non-empty `description`.
7. Every `enum` holds only strings.
8. The tool name matches `^[a-z0-9_]{1,64}$`.
9. The root has `"additionalProperties": false`.
   - Before adding it to a tool, grep that tool's run function. If the tool reads a key the schema
     does not declare, declare the key; do not skip the rule.

### 3.3 `part_build` schema

- `op`: `enum` of every op the `build` match in `server/src/tools/part.rs` accepts. Today these are
  box, cylinder, sphere, cone, extrude, revolve, union, cut, intersect, fillet, chamfer, place, hole,
  enclosure, lid and standoffs. Read the match; do not trust this list.
- `size`: `anyOf` `[number, string, array of number]`. Its description names when each is used:
  - an array for box and enclosure
  - a string such as `"M3"` for hole and standoffs
- `edges`: `anyOf` `[string enum ["all","vertical"], object {at_z:number, required ["at_z"]}]`.
- `points`: `anyOf` `[array of (array of number, 2 items), object {grid: array of number, corners:
  array of number}]`.
- `outline`: object with properties:
  - `circle` (array of number, 3 items)
  - `start` (array of number, 2 items)
  - `segments` (array of object `{line, arc, through}`, each an array of number with 2 items)
- Each step object and the root get `"additionalProperties": false`.
- The step properties stay the union of all ops' keys.
- `validate.rs` handles `anyOf` and `additionalProperties:false` exactly as in 3.1.
  - It still ignores keywords it does not know.

### 3.4 Tool name normalising (one shared helper used by `agent.rs` and `mcp.rs`)

- `pub(crate) fn find_tool<'a>(tools: &'a [Tool], name: &str) -> Option<&'a Tool>` in `agent.rs`.
- Try the exact name first. If that fails:
  1. Lowercase the name.
  2. Keep the text after the last `.`, then after the last `__`.
  3. Replace `-` with `_`.
  4. Match once more.
- The `ToolCall` event and the history keep the name exactly as the model sent it.

### 3.5 System prompt

- `system_prompt(project_name, tools)` includes a sentence only when every tool it names is in
  `tools`.
- Split the 3D-tool sentence so that each of these stands alone and drops out when its tool is
  missing:
  - the `part_build` part
  - the `freecad_build` part
  - the `blender_build` part
  - the `freecad_inspect` part
  - the `freecad_drawing` part
- Delete "Use KiCad tools only when the user works in KiCad."
- The `pcb_build`, `rf_calc`, `rf_antenna_sim` and `design_assemble` sentences follow the same rule.

### 3.6 Chat box layout (`web/index.html`, `web/main.ts`, `web/styles.css`)

- Move the three selects out of the header into `.composer-row`, left of Send.
- Change `<form id="model-pick">` to `<div id="model-pick" class="model-pick">`, because forms
  cannot nest. Delete the `submit` listener in `main.ts`.
- Remove the visible hint "Enter to send · Shift+Enter for a new line". Put the same text in the
  textarea's `title`.
- Keep the selects usable when no project is open. They are not disabled with the prompt.
- Make the selects compact:
  - 11px text, transparent background, border shown only on hover or focus
  - `#model-name` at most 180px wide
  - the row wraps (`flex-wrap`) when the panel is narrow
- Keep the `.warn` offline colour and every `title` and `aria-label`.
- The header keeps the brand, the project crumb and the Viewer/Design tabs.

## 4. Models: who does what

| Role | Model | `delegate_task` target |
|---|---|---|
| Code | GPT-6.1-Sol | codex `gpt-6.1-sol`, options `{"reasoningEffort":"high","serviceTier":"default"}` |
| Tests | Gemini 3.8 Flash | antigravity `gemini-3.8-flash-high` |
| Review, screenshots, version, commit | Orchestrator (Claude) | — |

Use a distinct `clientRequestId` per round.

## 5. Work order

- [x] Step 1: reference tests (Gemini)
- [x] Step 2: validator, schemas, errors, name lookup, prompt (GPT)
- [x] Step 3: chat box layout (GPT)
- [x] Step 4: release 0.15.1 (orchestrator)

### Step 1 brief: reference tests (send to gemini-3.8-flash-high)

> You write tests for Forge, a Rust server in `server/`. Read `AGENTS.md` and sections 3.1 to 3.5
> of `plan.md`. Do not edit any file outside `server/tests/`.
>
> Add these tests, each marked `#[ignore = "patch 0.15.1 not built yet"]`:
>
> 1. `server/tests/tools.rs` `every_tool_schema_is_portable`: walk every tool in
>    `forge_server::tools::all_tools()` and assert each rule in plan.md 3.2. On failure, print the
>    tool name and the JSON path of the bad node.
> 2. `server/tests/tools.rs` `validator_handles_any_of_and_unknown_keys`: call `validate` on small
>    hand-written schemas and assert these exact messages:
>    - `input.size must be a number, a string or an array`
>    - `input.size must be a number or a string`
>    - `input.lenght is not a known field`
>    Also assert that a matching `anyOf` branch gives no problems.
> 3. `server/tests/agent.rs` `weak_model_recovers_from_bad_calls`: use the scripted `Model` from
>    `server/tests/common/`. The model makes three calls in three turns:
>    - a `write_file` call whose input is `{"_unparsed":"{oops"}`
>    - a call named `functions.write-file` with a valid input
>    - a plain text reply
>    Assert the first tool result starts with `Arguments were not valid JSON: ` and ends with
>    `. Send the input as one JSON object.` Assert the second call wrote the file.
> 4. `server/tests/agent.rs` `unknown_tool_lists_available_tools`: a call to `launch_rocket` returns
>    `Unknown or unavailable tool "launch_rocket". Available tools: ` followed by the names of the
>    tools the test agent has, comma and space separated, in registry order, then `.`.
> 5. `server/tests/agent.rs` `prompt_names_only_available_tools`: build
>    `system_prompt("p", &subset)`, where `subset` is `all_tools()` without every tool whose name
>    starts with `freecad_`, `blender_` or `rf_antenna`. Assert the prompt contains `part_build`.
>    Assert it does not contain `freecad_`, `blender_build`, `rf_antenna_sim` or `KiCad tools`.
>
> If a function you need is private, write the test against the name in plan.md anyway. The
> implementer makes it public. Run `cargo test --workspace`. Every test other than yours must still
> pass, and yours must be ignored. Report the test names and the command output tail.

**Orchestrator check:**
- The diff touches only `server/tests/`.
- The strings match 3.1 exactly.
- `cargo test --workspace` passes with the new tests ignored.

### Step 2 brief: validator, schemas, errors, name lookup, prompt (send to gpt-6.1-sol)

> You code Forge, a Rust server in `server/`. Read `AGENTS.md` and all of `plan.md`, then implement
> sections 3.1 to 3.5:
> - `validate.rs`: `anyOf` and `additionalProperties:false`, with the exact messages.
> - `agent.rs` `execute_tool`: the bad-JSON message. Re-parse the `_unparsed` string with
>   `serde_json` to get the error text.
> - `agent.rs` `find_tool`, used by the agent loop and by `mcp.rs` in place of
>   `tools.iter().find(...)`.
> - The unknown-tool message with the list of available tool names.
> - The `part_build` schema in 3.3.
> - Portable schemas for every tool (3.2).
> - The conditional system prompt (3.5).
>
> Remove the `#[ignore]` marks from the five tests named in plan.md step 1, and make them pass
> without changing what they assert. Update existing tests that pinned the old unknown-tool sentence
> (`server/tests/mcp.rs`, `server/tests/agent.rs`) to the new full sentence.
>
> Do not change `model.rs`'s `_unparsed` storage. Keep every other error message word for word. No
> new crates. Done means `cargo test --workspace` passes and `cargo clippy --workspace` shows no new
> warnings. Report the diff summary and the test output tail.

**Orchestrator check:**
1. Read the whole diff.
2. Grep each tool's run function for keys missing from its schema.
3. Rerun `cargo test --workspace` 3 times.
4. Rerun `npm run typecheck`.
5. Dump `tools/list` from a running server: every node is typed and `op` has an enum.

### Step 3 brief: chat box layout (send to gpt-6.1-sol)

> You code Forge's web UI in `web/` (TypeScript, Vite, no framework). Read `AGENTS.md` and
> `plan.md` section 3.6, then implement it in `web/index.html`, `web/main.ts` and `web/styles.css`.
> Do not touch the server.
>
> Done means:
> - `npm run typecheck` and `npm run build` pass.
> - The provider, model and thinking selects work from the chat box: changing them still posts to
>   `/api/model`.
> - The thinking select still shows only for the claude provider.
>
> Report the diff summary.

**Orchestrator check:**
1. Run the screenshot script (`/tmp/shotkit/shot.mjs`; recreate it with `playwright-core` in `/tmp`
   if it is missing) against an offline server at 1440×900 and at 1000×700.
2. Confirm that:
   - the selects sit left of Send and the row wraps cleanly
   - the header has no selects
   - switching the provider still works
3. Send the screenshots to the user.

### Step 4: release 0.15.1 (orchestrator)

1. Run `npm version patch --no-git-tag-version` and mirror the version in `server/Cargo.toml`.
2. Add a `## 0.15.1 — <date>` entry to CHANGELOG.md:
   - model picker moved into the chat box
   - portable tool schemas
   - clear errors for bad JSON, unknown tools and unknown keys
   - the prompt names only available tools
3. Update `docs/ARCHITECTURE.md` where it describes the header or the tool path.
4. Run `cargo test --workspace` and `npm run typecheck`, then commit.
5. Optional live check: for each provider with a key in `~/.config/forge/env`, send "Design a
   60 × 40 mm enclosure with M3 standoffs" and confirm that `part_build` succeeds within 3 calls.

## 6. Pitfalls (already inside the briefs; listed here for the reviewer)

- `additionalProperties:false` on a tool that reads an undeclared key makes `execute_tool` reject
  valid calls from models. Tests that call the tool directly skip validation and will not catch it.
  Grep the run function for each tool.
- The `op` enum makes the validator reject unknown ops before `part_build` runs. Tests that call
  `part_build` directly still see `step <index>: unknown op "<op>"`.
- Nested `<form>` elements are invalid HTML. `model-pick` must become a `div`.
- Never call `std::env::set_var` in tests. Never hold a `std::sync::Mutex` guard across `.await`.
