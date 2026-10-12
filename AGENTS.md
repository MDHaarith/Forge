# Working on Forge

- Run `npm run typecheck` and `npm test` before committing. Tests must not need the network or an API key; use a scripted `Model`.
- Server code runs on Node's built-in TypeScript type stripping: import with `.ts` extensions, use `import type` for types, and no enums, namespaces or constructor parameter properties.
- Tools never touch paths outside the project: resolve every path through `projectPath`.
- Every change to app code bumps the version in package.json (`npm version patch|minor|major --no-git-tag-version`) and adds a `## <version> — <YYYY-MM-DD>` entry at the top of CHANGELOG.md.
