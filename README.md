# Kuizer

**Kuizer** (noun) /ˈkœy.zər/ — Frisian-derived, "one who cleans, restores order, or brings something back to a proper state."

A deterministic CLI code hygiene tool that enforces a strict, opinionated TypeScript and JavaScript style. Kuizer transforms codebases by applying structural refactors such as converting interfaces to types, arrow functions to named function declarations, and normalizing patterns to a consistent, performance-oriented standard.

Designed for large, long-lived codebases, Kuizer operates recursively, respects ignore files, and applies changes safely and predictably. Rather than linting or formatting, it performs semantic cleanup to align code with a single, coherent style philosophy. Kuizer is intended as a one-command cleanup and enforcement tool, not a configurable rule engine.

## Dead Code Detection

Kuizer includes a dead code detector that identifies:

- **Unused files**: Entire source files with no imports from the entry point
- **Unused exports**: Exported functions, classes, types that are never imported anywhere

### Test Fixture

The `scaffold-e2e-tests.sh` script generates a sample project (`tests/fixtures/sample/`) for testing dead code detection. It contains:

**Live code:**
- `src/index.ts` — entry point
- `src/lib/greet.ts` — `greet` function used by entry point
- `src/lib/logger.ts` — `Logger` class used by entry point
- `src/types/index.ts` — `ID` and `Result` types used by entry point

**Dead code (unused files):**
- `src/utils/format.ts` — never imported
- `src/utils/math.ts` — never imported
- `src/components/Button.ts` — never imported
- `src/components/Modal.ts` — never imported

**Dead code (unused exports):**
- `src/lib/greet.ts`: `shout`
- `src/lib/logger.ts`: `FileLogger`
- `src/utils/format.ts`: `formatDate`, `formatBytes`, `formatPercent`
- `src/types/index.ts`: `Timestamp`, `PaginatedResponse`

Run `./scaffold-e2e-tests.sh` to regenerate the fixture.

### Verification

Run the e2e tests to verify Kuizer correctly detects the dead code:

```bash
bun test tests/e2e.test.ts
```

This validates:
- `unusedFiles` detects: `format.ts`, `math.ts`, `Button.ts`, `Modal.ts`
- `unusedExports` detects: `shout`, `FileLogger`, `formatDate`, `formatBytes`, `formatPercent`, `Timestamp`, `PaginatedResponse`
