# Working on Forge

Forge is edited by AI agents. Read this file, then `docs/ARCHITECTURE.md` (map of every file and how a
chat flows), then `docs/ROADMAP.md` (what to build next and the rule that Forge must not depend on
CAD/EDA apps, Blender or other installed programs).
If `plan.md` exists at the repo root, it is the current work plan: follow it and tick its boxes.

## Before you commit
- Run `cargo test --workspace` and `npm run typecheck`. Both must pass with no warnings.
- Tests must not need the network, an API key or the real `claude`; use a scripted `Model` or a fake
  `claude` script (see `server/tests/common/`). Tests that need an external program skip with
  `eprintln!("skipped: …")` when it is missing.
- The desktop app lives in `desktop/` (Rust, Tauri 2). Run `sh scripts/stage-app.sh` once so the bundled
  `build/app` folder exists, or `cargo test --workspace` cannot build it.
- Every change to app code bumps the version in package.json
  (`npm version patch|minor|major --no-git-tag-version`), mirrors it in `server/Cargo.toml`, and adds a
  `## <version> — <YYYY-MM-DD>` entry at the top of CHANGELOG.md.
- When you add, move or delete a file or route, update `docs/ARCHITECTURE.md` in the same change.
  When you finish or re-plan roadmap work, update `docs/ROADMAP.md`.

## Rules the tests will not always catch
- Tools never touch paths outside the project: resolve every path through `project_path` in
  `server/src/tools/paths.rs`. Never `canonicalize` (files may not exist yet).
- Run programs only through `tools/exec.rs` `run` (no shell, timeout, output cap). Never block the async
  runtime: wrap `which`/`flatpak` probes and other slow sync calls in `tokio::task::spawn_blocking`.
- Configuration comes from `Settings` (built once from env in `lib.rs`). Never call `std::env::set_var`,
  especially in tests (they run in parallel).
- Files the UI reads (`models/*.json`, `design/*.json`) must look like JavaScript output: keep key order
  (serde_json `preserve_order`) and format numbers with `server/src/jsfmt.rs` (`5` not `5.0`, `0` not `-0`).
- Keep error messages word for word; the agent prompt and tests rely on them.
- Dependencies: add a crate only when it removes real work; say why in the commit message.
- Never hold a `std::sync::Mutex` guard across `.await`.
- Mark a deliberate limit with a `shortcut: <the limit>, <when to upgrade>` comment.

## Working with several AI agents
- One agent edits the checkout at a time. Parallel agents in the same folder overwrite each other.
- Give each agent a written brief: the goal, the files, the pitfalls above, and "done means" with the
  exact test commands. Review its diff and rerun the tests yourself before committing.

<!-- caveman-begin -->
Respond terse like smart caveman. All technical substance stay. Only fluff die.

Rules:
- Answer first: Answer, then reason, then next step.
- Kill ceremony: No greeting, hedging, pleasantries, recap, or closer.
- Short word: "fix" not "implement a solution for".
- Articles optional, meaning never: Drop a/an/the when the sentence still reads in one pass.
- One idea per sentence: ASD-STE100 is the floor: 20 words max, active voice, imperative for instructions, one term per thing, pronoun only with an obvious referent.
- Payload verbatim: Code blocks unchanged.
- Tool runs: bounded status: No text between routine calls.
- User's language: Compress the style, not the language.
- Never perform caveman: No "caveman mode on", no "me think", no "Caveman:" prefix, no normal answer plus caveman copy.

Switch: /caveman (default), /ultracave (fragments, each fact once), /megacave (Classical Chinese 文言文)
Stop: "stop caveman" or "normal mode"

Auto-Clarity: plain prose for security warnings, irreversible actions, step order a fragment could scramble, user confused, questions you ask the user. Resume after.

Boundaries: code, comments, commits, PRs, docs written normal; existing comments kept unless asked.
Floor: code, commands, paths, numbers and error strings verbatim; never drop not/never/no/only.
<!-- caveman-end -->
