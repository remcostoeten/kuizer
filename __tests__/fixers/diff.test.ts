import { describe, expect, it } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { renderPlanDiff } from "../../src/fixers/diff.js";
import type { FixPlan } from "../../src/fixers/types.js";

function makeFile(path: string, contents: string): string {
  const cwd = mkdtempSync(join(tmpdir(), "kuizer-fix-diff-"));
  const target = join(cwd, path);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, contents);
  return target;
}

describe("renderPlanDiff", () => {
  it("renders delete-file previews", () => {
    const file = makeFile("src/dead.ts", "export const dead = true\n");

    try {
      const plan: FixPlan = {
        supported: true,
        finding: {
          detectorId: "unusedFiles",
          severity: "warn",
          file,
          line: 1,
          col: 1,
          message: "File is unreachable from any entry point",
          fixable: false,
        },
        edits: [
          {
            kind: "delete-file",
            file,
            description: "Delete unreachable file",
          },
        ],
        safety: "destructive",
        reason: "File is unreachable and can be removed as a whole-file deletion",
      };

      const diff = renderPlanDiff(plan);
      expect(diff).toContain("---");
      expect(diff).toContain("+++ (deleted)");
      expect(diff).toContain("- export const dead = true");
    } finally {
      rmSync(dirname(dirname(file)), { recursive: true, force: true });
    }
  });
});
