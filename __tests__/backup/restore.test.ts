import { describe, expect, it } from "bun:test";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { createSnapshot } from "../../src/backup/create.js";
import { restoreSnapshot } from "../../src/backup/restore.js";
import { defaults } from "../../src/config/defaults.js";
import type { Config } from "../../src/types.js";
import type { PlannedEdit } from "../../src/fixers/types.js";

function makeWorkspace(): string {
  return mkdtempSync(join(tmpdir(), "kuizer-restore-"));
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

describe("restoreSnapshot", () => {
  it("restores files from a manifest path", () => {
    const cwd = makeWorkspace();
    const config = makeConfig(cwd);
    const file = join(cwd, "src/dead.ts");

    try {
      writeFile(file, "export const dead = true\n");
      writeFile(config.tsconfig, JSON.stringify({ compilerOptions: {}, include: ["src/**/*.ts"] }));

      const edits: readonly PlannedEdit[] = [
        { kind: "delete-file", file, description: "Delete unreachable file" },
      ];

      const snapshot = createSnapshot(edits, config);
      rmSync(file);

      const result = restoreSnapshot(snapshot.manifestPath);

      expect(result.manifestPath).toBe(snapshot.manifestPath);
      expect(result.snapshotId).toBeTruthy();
      expect(result.restored).toEqual([file]);
      expect(readFileSync(file, "utf8")).toBe("export const dead = true\n");
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("fails when a required snapshot file is missing", () => {
    const cwd = makeWorkspace();
    const config = makeConfig(cwd);
    const file = join(cwd, "src/nested/dead.ts");
    const otherFile = join(cwd, "src/live.ts");

    try {
      writeFile(file, "export const dead = true\n");
      writeFile(otherFile, "export const live = true\n");
      writeFile(config.tsconfig, JSON.stringify({ compilerOptions: {}, include: ["src/**/*.ts"] }));

      const edits: readonly PlannedEdit[] = [
        { kind: "delete-file", file, description: "Delete unreachable file" },
        { kind: "delete-file", file: otherFile, description: "Delete other file" },
      ];

      const snapshot = createSnapshot(edits, config);
      const snapshotFile = join(dirname(snapshot.manifestPath), "files", "src", "nested", "dead.ts");
      rmSync(snapshotFile);
      rmSync(file);
      rmSync(otherFile);

      expect(() => restoreSnapshot(snapshot.manifestPath)).toThrow("Snapshot file is missing");
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("restores files from a snapshot id when outputDir is provided", () => {
    const cwd = makeWorkspace();
    const config = makeConfig(cwd);
    const file = join(cwd, "src/dead.ts");

    try {
      writeFile(file, "export const dead = true\n");
      writeFile(config.tsconfig, JSON.stringify({ compilerOptions: {}, include: ["src/**/*.ts"] }));

      const edits: readonly PlannedEdit[] = [
        { kind: "delete-file", file, description: "Delete unreachable file" },
      ];

      const snapshot = createSnapshot(edits, config);
      const manifest = JSON.parse(readFileSync(snapshot.manifestPath, "utf8")) as { id: string };
      rmSync(file);

      const result = restoreSnapshot(manifest.id, {
        cwd,
        outputDir: config.outputDir,
      });

      expect(result.manifestPath).toBe(snapshot.manifestPath);
      expect(result.snapshotId).toBe(manifest.id);
      expect(result.restored).toEqual([file]);
      expect(readFileSync(file, "utf8")).toBe("export const dead = true\n");
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });
});
