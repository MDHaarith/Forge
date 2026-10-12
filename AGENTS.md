# Working on Forge

- Run `npm run typecheck` and `npm test` before committing. Tests must not need the network or an API key; use a scripted `Model`.
- Server code runs on Node's built-in TypeScript type stripping: import with `.ts` extensions, use `import type` for types, and no enums, namespaces or constructor parameter properties.
- The desktop app lives in `desktop/` (Rust, Tauri 2). After changing it, run `cargo test` there (it needs `sh scripts/stage-app.sh` once, so the bundled `build/app` folder exists).
- Tools never touch paths outside the project: resolve every path through `projectPath`.
- Every change to app code bumps the version in package.json (`npm version patch|minor|major --no-git-tag-version`) and adds a `## <version> — <YYYY-MM-DD>` entry at the top of CHANGELOG.md.

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
