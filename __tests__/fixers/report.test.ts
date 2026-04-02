import { afterEach, describe, expect, it, spyOn } from "bun:test";
import { reportFixPlan } from "../../src/fixers/report.js";
import type { FixPlanningResult } from "../../src/fixers/types.js";

afterEach(() => {
  spyOn(process.stdout, "write").mockRestore();
});

describe("reportFixPlan", () => {
  it("prints supported and unsupported summaries", () => {
    let output = "";
    spyOn(process.stdout, "write").mockImplementation((chunk: string | Uint8Array) => {
      output += String(chunk);
      return true;
    });

    const result: FixPlanningResult = {
      supported: [
        {
          supported: true,
          finding: {
            detectorId: "unusedFiles",
            severity: "warn",
            file: "/src/dead.ts",
            line: 1,
            col: 1,
            message: "File is unreachable from any entry point",
            fixable: false,
          },
          edits: [
            {
              kind: "delete-file",
              file: "/src/dead.ts",
              description: "Delete unreachable file",
            },
          ],
          safety: "destructive",
          reason: "File is unreachable and can be removed as a whole-file deletion",
        },
      ],
      unsupported: [
        {
          supported: false,
          finding: {
            detectorId: "unusedExports",
            severity: "warn",
            file: "/src/utils.ts",
            line: 1,
            col: 1,
            message: "Exported symbol 'dead' has 0 consumers",
            symbol: "dead",
            fixable: false,
          },
          reason: "No fixer planner is registered for detector 'unusedExports'",
        },
      ],
    };

    reportFixPlan(result, { dryRun: true });

    expect(output).toContain("Dry-run fix plan");
    expect(output).toContain("Unsupported fixes");
    expect(output).toContain("1 supported");
    expect(output).toContain("1 unsupported");
  });
});
