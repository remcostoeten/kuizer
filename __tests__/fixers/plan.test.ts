import { describe, expect, it } from "bun:test";
import { planFixes } from "../../src/fixers/plan.js";
import { makeConfig, makeCtx } from "../detectors/fixture.js";
import type { Finding } from "../../src/types.js";

function makeUnusedFileFinding(
  file: string,
  overrides: Partial<Finding> = {},
): Finding {
  return {
    detectorId: "unusedFiles",
    severity: "warn",
    file,
    line: 1,
    col: 1,
    message: "File is unreachable from any entry point",
    fixable: false,
    ...overrides,
  };
}

describe("planFixes", () => {
  it("plans a delete-file edit for unreachable unused files", () => {
    const ctx = makeCtx(
      { "/src/dead.ts": "", "/src/live.ts": "" },
      { "/src/live.ts": "" },
    );
    const result = planFixes(
      [makeUnusedFileFinding("/src/dead.ts")],
      ctx,
      makeConfig(),
    );

    expect(result.supported).toHaveLength(1);
    expect(result.unsupported).toHaveLength(0);
    expect(result.supported[0]?.safety).toBe("destructive");
    expect(result.supported[0]?.edits).toEqual([
      {
        kind: "delete-file",
        file: "/src/dead.ts",
        description: "Delete unreachable file",
      },
    ]);
  });

  it("refuses to plan unusedFiles deletion when dynamic import uncertainty exists", () => {
    const ctx = makeCtx(
      { "/src/dead.ts": "", "/src/live.ts": "" },
      { "/src/live.ts": "" },
    );
    const result = planFixes(
      [
        makeUnusedFileFinding("/src/dead.ts", {
          context: {
            analysisBoundaryCount: 1,
            hasDynamicImportBoundary: true,
          },
          message: "File is unreachable from any statically resolved entry point",
        }),
      ],
      ctx,
      makeConfig(),
    );

    expect(result.supported).toHaveLength(0);
    expect(result.unsupported).toHaveLength(1);
    expect(result.unsupported[0]?.reason).toContain("Dynamic import");
  });

  it("marks findings without a registered planner as unsupported", () => {
    const ctx = makeCtx(
      { "/src/utils.ts": "export const dead = 1" },
      { "/src/utils.ts": "export const dead = 1" },
    );
    const result = planFixes(
      [
        {
          detectorId: "unusedExports",
          severity: "warn",
          file: "/src/utils.ts",
          line: 1,
          col: 1,
          message: "Exported symbol 'dead' has 0 consumers",
          symbol: "dead",
          fixable: false,
        },
      ],
      ctx,
      makeConfig(),
    );

    expect(result.supported).toHaveLength(0);
    expect(result.unsupported).toHaveLength(1);
    expect(result.unsupported[0]?.reason).toContain("No fixer planner");
  });
});
