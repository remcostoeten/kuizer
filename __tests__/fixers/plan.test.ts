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

function makeUnusedExportFinding(
  detectorId: Finding["detectorId"],
  file: string,
  symbol: string,
  overrides: Partial<Finding> = {},
): Finding {
  return {
    detectorId,
    severity: "warn",
    file,
    line: 1,
    col: 1,
    message: `Exported symbol '${symbol}' has 0 consumers`,
    symbol,
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
          detectorId: "unknownDetector",
          severity: "warn",
          file: "/src/utils.ts",
          line: 1,
          col: 1,
          message: "Some finding",
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

  it("plans a delete-file edit for simple standalone unused exports", () => {
    const ctx = makeCtx(
      { "/src/utils.ts": "export function dead() { return 1 }\n" },
      { "/src/utils.ts": "export function dead() { return 1 }\n" },
    );
    const result = planFixes(
      [makeUnusedExportFinding("unusedExports", "/src/utils.ts", "dead")],
      ctx,
      makeConfig(),
    );

    expect(result.supported).toHaveLength(1);
    expect(result.unsupported).toHaveLength(0);
    expect(result.supported[0]?.edits).toEqual([
      {
        kind: "delete-file",
        file: "/src/utils.ts",
        description: "Delete unused export 'dead'",
      },
    ]);
  });

  it("marks grouped variable unusedExports as unsupported", () => {
    const ctx = makeCtx(
      { "/src/utils.ts": "export const a = 1, b = 2\n" },
      { "/src/utils.ts": "export const a = 1, b = 2\n" },
    );
    const result = planFixes(
      [makeUnusedExportFinding("unusedExports", "/src/utils.ts", "a")],
      ctx,
      makeConfig(),
    );

    expect(result.supported).toHaveLength(0);
    expect(result.unsupported).toHaveLength(1);
    expect(result.unsupported[0]?.reason).toContain("Only simple whole-declaration exports");
  });

  it("keeps unusedExports unsupported even with dynamic import uncertainty", () => {
    const ctx = makeCtx(
      { "/src/utils.ts": "export function dead() { return 1 }\n" },
      { "/src/utils.ts": "export function dead() { return 1 }\n" },
    );
    const result = planFixes(
      [
        makeUnusedExportFinding("unusedExports", "/src/utils.ts", "dead", {
          context: {
            analysisBoundaryCount: 1,
            hasDynamicImportBoundary: true,
          },
          message: "Exported symbol 'dead' has 0 statically resolved consumers",
        }),
      ],
      ctx,
      makeConfig(),
    );

    expect(result.supported).toHaveLength(0);
    expect(result.unsupported).toHaveLength(1);
    expect(result.unsupported[0]?.reason).toContain("Dynamic import");
  });

  it("marks default export unusedExports as unsupported", () => {
    const ctx = makeCtx(
      { "/src/utils.ts": "export default function dead() { return 1 }\n" },
      { "/src/utils.ts": "export default function dead() { return 1 }\n" },
    );
    const result = planFixes(
      [makeUnusedExportFinding("unusedExports", "/src/utils.ts", "default")],
      ctx,
      makeConfig(),
    );

    expect(result.supported).toHaveLength(0);
    expect(result.unsupported).toHaveLength(1);
    expect(result.unsupported[0]?.reason).toContain("Default exports");
  });

  it("marks barrel re-export unusedExports as unsupported", () => {
    const ctx = makeCtx(
      {
        "/src/origin.ts": "export const dead = 1\n",
        "/src/barrel.ts": "export { dead } from '/src/origin'\n",
      },
      {
        "/src/origin.ts": "export const dead = 1\n",
        "/src/barrel.ts": "export { dead } from '/src/origin'\n",
      },
    );
    const result = planFixes(
      [makeUnusedExportFinding("unusedExports", "/src/barrel.ts", "dead")],
      ctx,
      makeConfig(),
    );

    expect(result.supported).toHaveLength(0);
    expect(result.unsupported).toHaveLength(1);
    expect(result.unsupported[0]?.reason).toContain("Only simple whole-declaration exports");
  });

  it.each([
    "unusedFunctions",
    "unusedTypes",
    "unusedComponents",
  ] satisfies Finding["detectorId"][])(
    "routes %s findings through the unused export planner when the declaration is supported",
    (detectorId) => {
      const file = detectorId === "unusedComponents" ? "/src/component.tsx" : "/src/utils.ts";
      const source =
        detectorId === "unusedComponents"
          ? "export function Button() { return null }\n"
          : "export function dead() { return 1 }\n";
      const symbol = detectorId === "unusedComponents" ? "Button" : "dead";
      const ctx = makeCtx({ [file]: source }, { [file]: source });
      const result = planFixes(
        [makeUnusedExportFinding(detectorId, file, symbol)],
        ctx,
        makeConfig(),
      );

      expect(result.supported).toHaveLength(1);
      expect(result.unsupported).toHaveLength(0);
      expect(result.supported[0]?.edits).toEqual([
        {
          kind: "delete-file",
          file,
          description: `Delete unused export '${symbol}'`,
        },
      ]);
    },
  );

  it.each([
    "unusedExports",
    "unusedDefaults",
  ] satisfies Finding["detectorId"][])(
    "keeps default exports unsupported for %s findings",
    (detectorId) => {
      const ctx = makeCtx(
        { "/src/utils.ts": "export default function dead() { return 1 }\n" },
        { "/src/utils.ts": "export default function dead() { return 1 }\n" },
      );
      const result = planFixes(
        [makeUnusedExportFinding(detectorId, "/src/utils.ts", "default")],
        ctx,
        makeConfig(),
      );

      expect(result.supported).toHaveLength(0);
      expect(result.unsupported).toHaveLength(1);
      expect(result.unsupported[0]?.reason).toContain("Default exports");
    },
  );
});
