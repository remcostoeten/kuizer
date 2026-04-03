import { afterEach, describe, expect, it, spyOn } from "bun:test";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { createSnapshot } from "../../src/backup/create.js";
import { main } from "../../src/cli/index.js";
import { defaults } from "../../src/config/defaults.js";
import type { Config } from "../../src/types.js";
import type { PlannedEdit } from "../../src/fixers/types.js";

function makeWorkspace(): string {
  return mkdtempSync(join(tmpdir(), "kuizer-restore-cli-"));
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

afterEach(() => {
  spyOn(process.stdout, "write").mockRestore();
});

describe("kuizer restore", () => {
  it("restores a snapshot by manifest path", async () => {
    const cwd = makeWorkspace();
    const config = makeConfig(cwd);
    const file = join(cwd, "src/dead.ts");
    let output = "";

    try {
      writeFile(file, "export const dead = true\n");
      writeFile(config.tsconfig, JSON.stringify({ compilerOptions: {}, include: ["src/**/*.ts"] }));

      const edits: readonly PlannedEdit[] = [
        { kind: "delete-file", file, description: "Delete unreachable file" },
      ];

      const snapshot = createSnapshot(edits, config);
      rmSync(file);

      spyOn(process.stdout, "write").mockImplementation((chunk: string | Uint8Array) => {
        output += String(chunk);
        return true;
      });

      const code = await main(["restore", "--snapshot", snapshot.manifestPath]);

      expect(code).toBe(0);
      expect(existsSync(file)).toBeTrue();
      expect(readFileSync(file, "utf8")).toBe("export const dead = true\n");
      expect(output).toContain("Restored 1 file");
      expect(output).toContain(snapshot.manifestPath);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("restores a snapshot by id with config outputDir", async () => {
    const cwd = makeWorkspace();
    const config = makeConfig(cwd);
    const file = join(cwd, "src/dead.ts");
    let output = "";

    try {
      writeFile(file, "export const dead = true\n");
      writeFile(config.tsconfig, JSON.stringify({ compilerOptions: {}, include: ["src/**/*.ts"] }));
      writeFile(
        join(cwd, "kuizer.config.json"),
        JSON.stringify({
          tsconfig: "./tsconfig.json",
          outputDir: "./.kuizer",
        }),
      );

      const edits: readonly PlannedEdit[] = [
        { kind: "delete-file", file, description: "Delete unreachable file" },
      ];

      const snapshot = createSnapshot(edits, config);
      const manifest = JSON.parse(readFileSync(snapshot.manifestPath, "utf8")) as { id: string };
      rmSync(file);

      spyOn(process.stdout, "write").mockImplementation((chunk: string | Uint8Array) => {
        output += String(chunk);
        return true;
      });

      const code = await main([
        "restore",
        "--config",
        join(cwd, "kuizer.config.json"),
        "--snapshot",
        manifest.id,
      ]);

      expect(code).toBe(0);
      expect(existsSync(file)).toBeTrue();
      expect(readFileSync(file, "utf8")).toBe("export const dead = true\n");
      expect(output).toContain("Restored 1 file");
      expect(output).toContain(snapshot.manifestPath);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });
});
