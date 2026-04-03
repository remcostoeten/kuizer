# Repository Guidelines

## Project Structure & Module Organization

Kuizer is a Bun-based TypeScript CLI. Source lives in `src/`, split by responsibility: `cli/` for command entrypoints, `engine/` for analysis bootstrap, `detectors/` for findings, `fixers/` for plan/apply flows, `reporters/` for output, `backup/` for snapshot restore, and `config/` for config loading. Build artifacts are generated into `dist/`. Tests live in `__tests__/`, with reusable fixture projects in `tests/fixtures/`. Helper scripts belong in `scripts/`.

## Build, Test, and Development Commands

- `bun install --frozen-lockfile`: install dependencies exactly as locked.
- `bun run build`: bundle the CLI from `src/cli/index.ts` into `dist/` with `tsup`.
- `bun run dev`: rebuild on file changes during local development.
- `bun run typecheck`: run strict TypeScript checks with `tsc --noEmit`.
- `bun run test`: execute the Bun test suite.
- `bash scripts/test-runner.sh`: run the CI-style test wrapper used in GitHub Actions.
- `bun run lint` / `bun run lint:fix`: check or auto-fix issues with `oxlint`.
- `bun run fmt` / `bun run fmt:check`: apply or verify formatting with `oxfmt`.

## Coding Style & Naming Conventions

Formatting is enforced by `oxfmt`: tabs, 4-column width, single quotes, no semicolons, trailing commas where valid, and LF endings. Prefer small, focused modules grouped by feature. Use lowercase kebab-case filenames such as `unused-exports.ts` and `restore-command.test.ts`. Keep CLI-facing code in `src/cli/` thin and push logic into feature modules.

## Testing Guidelines

Write tests with Bun and place them in `__tests__/` using the `*.test.ts` suffix. Prefer fixture-driven integration coverage for CLI behavior and keep supporting sample projects in `tests/fixtures/`. If fixture layout changes, regenerate scaffolding with `bash scripts/scaffold-e2e-tests.sh`. Before opening a PR, run `bun run typecheck` and `bash scripts/test-runner.sh`.

## Commit & Pull Request Guidelines

Follow the existing history: short imperative subjects, usually with Conventional Commit prefixes like `feat(detectors): ...`, `chore: ...`, or `fix(reporters): ...`. Keep each commit scoped to one change. PRs should describe user-visible behavior, list affected commands or detectors, link related issues when relevant, and include terminal output or snapshots if CLI output changes.

## Agent Notes

Do not edit `dist/` by hand; rebuild it from source. Avoid reverting unrelated changes in a dirty worktree. If you add a detector, fixer, or reporter, update its registry and cover it with tests in `__tests__/`.
