import { describe, expect, it } from "bun:test";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { createSnapshot } from "../../src/backup/create.js";
import { defaults } from "../../src/config/defaults.js";
import type { Config } from "../../src/types.js";
import type { PlannedEdit } from "../../src/fixers/types.js";

function makeWorkspace(): string {
  return mkdtempSync(join(tmpdir(), "kuizer-snapshot-"));
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

describe("createSnapshot", () => {
  it("stores original file contents in the snapshot files directory", () => {
    const cwd = makeWorkspace();
    const fileA = join(cwd, "src/deep/a.ts");
    const fileB = join(cwd, "src/b.ts");
    const tsconfig = join(cwd, "tsconfig.json");

    try {
      writeFile(fileA, "export const a = 1\n");
      writeFile(fileB, "export const b = 2\n");
      writeFile(tsconfig, JSON.stringify({ compilerOptions: {}, include: ["src/**/*.ts"] }));

      const edits: readonly PlannedEdit[] = [
        { kind: "delete-file", file: fileA, description: "Delete unreachable file" },
        { kind: "delete-file", file: fileB, description: "Delete unreachable file" },
      ];

      const snapshot = createSnapshot(edits, makeConfig(cwd));
      const storedA = join(snapshot.snapshotDir, "files", fileA.slice(cwd.length + 1));
      const storedB = join(snapshot.snapshotDir, "files", fileB.slice(cwd.length + 1));

      expect(readFileSync(storedA, "utf8")).toBe("export const a = 1\n");
      expect(readFileSync(storedB, "utf8")).toBe("export const b = 2\n");
      const manifest = JSON.parse(readFileSync(snapshot.manifestPath, "utf8")) as {
        id: string;
        operation: string;
        createdAt: string;
        root?: string;
        files: readonly string[];
      };

      expect(manifest.operation).toBe("fix");
      expect(manifest.id).toContain("-fix");
      expect(manifest.createdAt).toBeTruthy();
      expect(manifest.root).toBe(cwd);
      expect(manifest.files).toEqual([fileB, fileA]);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("rejects files outside the snapshot root", () => {
    const cwd = makeWorkspace();
    const external = makeWorkspace();
    const file = join(external, "src/outside.ts");
    const tsconfig = join(cwd, "tsconfig.json");

    try {
      writeFile(file, "export const outside = true\n");
      writeFile(tsconfig, JSON.stringify({ compilerOptions: {}, include: ["src/**/*.ts"] }));

      expect(() =>
        createSnapshot([{ kind: "delete-file", file, description: "Delete outside file" }], makeConfig(cwd)),
      ).toThrow("outside the snapshot root");
    } finally {
      rmSync(cwd, { recursive: true, force: true });
      rmSync(external, { recursive: true, force: true });
    }
  });
});
