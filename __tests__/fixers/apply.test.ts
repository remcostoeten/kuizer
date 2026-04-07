import { describe, expect, it } from "bun:test";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { applyFixPlans } from "../../src/fixers/apply.js";
import { defaults } from "../../src/config/defaults.js";
import type { Config } from "../../src/types.js";
import type { FixPlan, UnsupportedFix } from "../../src/fixers/types.js";

function makeWorkspace(): string {
  return mkdtempSync(join(tmpdir(), "kuizer-fix-apply-"));
}

function makeConfig(cwd: string): Config {
  return {
    ...defaults,
    tsconfig: join(cwd, "tsconfig.json"),
    outputDir: join(cwd, ".kuizer"),
  };
}

function writeFile(path: string, contents: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, contents);
}

describe("applyFixPlans", () => {
  it("deletes files and creates a snapshot manifest", () => {
    const cwd = makeWorkspace();
    const file = join(cwd, "src/dead.ts");
    const tsconfig = join(cwd, "tsconfig.json");

    try {
      writeFile(file, "export const dead = true\n");
      writeFile(tsconfig, JSON.stringify({ compilerOptions: {}, include: ["src/**/*.ts"] }));

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

      const result = applyFixPlans([plan], [] satisfies readonly UnsupportedFix[], makeConfig(cwd), {
        backup: true,
      });

      expect(existsSync(file)).toBeFalse();
      expect(result.snapshotPath).toBeDefined();
      expect(result.snapshotPath && existsSync(result.snapshotPath)).toBeTrue();
      expect(result.snapshotPath?.includes("manifest.json")).toBeTrue();

      const manifest = JSON.parse(readFileSync(result.snapshotPath!, "utf8"));
      expect(manifest.operation).toBe("fix");
      expect(manifest.files).toEqual([file]);

      const snapshotFile = join(dirname(result.snapshotPath!), "files", file.slice(cwd.length + 1));
      expect(existsSync(snapshotFile)).toBeTrue();
      expect(readFileSync(snapshotFile, "utf8")).toBe("export const dead = true\n");
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("skips snapshots when backup is disabled", () => {
    const cwd = makeWorkspace();
    const file = join(cwd, "src/dead.ts");
    const tsconfig = join(cwd, "tsconfig.json");

    try {
      writeFile(file, "export const dead = true\n");
      writeFile(tsconfig, JSON.stringify({ compilerOptions: {}, include: ["src/**/*.ts"] }));

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

      const result = applyFixPlans([plan], [] satisfies readonly UnsupportedFix[], makeConfig(cwd), {
        backup: false,
      });

      expect(existsSync(file)).toBeFalse();
      expect(result.snapshotPath).toBeUndefined();
      expect(existsSync(join(cwd, ".kuizer"))).toBeFalse();
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("applies multiple declaration edits in descending line order", () => {
    const cwd = makeWorkspace();
    const file = join(cwd, "src/exports.ts");
    const tsconfig = join(cwd, "tsconfig.json");

    try {
      writeFile(
        file,
        [
          "export const first = 1",
          "",
          "const keep = true",
          "",
          "export const second = 2",
          "",
        ].join("\n")
      );
      writeFile(tsconfig, JSON.stringify({ compilerOptions: {}, include: ["src/**/*.ts"] }));

      const plans: FixPlan[] = [
        {
          supported: true,
          finding: {
            detectorId: "unusedExports",
            severity: "warn",
            file,
            line: 1,
            col: 1,
            message: "Unused export 'first'",
            symbol: "first",
            kind: "const",
            fixable: true,
          },
          edits: [
            {
              kind: "delete-declaration",
              file,
              exportName: "first",
              startLine: 1,
              endLine: 1,
              description: "Delete first",
            },
          ],
          safety: "cautious",
          reason: "Delete first",
        },
        {
          supported: true,
          finding: {
            detectorId: "unusedExports",
            severity: "warn",
            file,
            line: 5,
            col: 1,
            message: "Unused export 'second'",
            symbol: "second",
            kind: "const",
            fixable: true,
          },
          edits: [
            {
              kind: "delete-declaration",
              file,
              exportName: "second",
              startLine: 5,
              endLine: 5,
              description: "Delete second",
            },
          ],
          safety: "cautious",
          reason: "Delete second",
        },
      ];

      applyFixPlans(plans, [] satisfies readonly UnsupportedFix[], makeConfig(cwd), {
        backup: false,
      });

      expect(readFileSync(file, "utf8")).toBe(["const keep = true", ""].join("\n"));
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });
});
