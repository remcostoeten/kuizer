import { describe, it, expect } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { loadConfig } from "../../src/config/index.js";

describe("loadConfig", () => {
  it("returns fully resolved config with defaults when no file exists", async () => {
    const config = await loadConfig("/tmp");

    expect(config.framework).toBe("none");
    expect(config.tsconfig).toBe("/tmp/tsconfig.json");
    expect(config.outputDir).toBe("/tmp/.kuizer");
    expect(config.entrypoints).toBeArray();
  });

  it("loads an explicit config path", async () => {
    const cwd = mkdtempSync(join(tmpdir(), "kuizer-config-"));
    const configPath = join(cwd, "kuizer.config.js");

    try {
      writeFileSync(
        configPath,
        [
          "module.exports = {",
          "  framework: 'nextjs-pages',",
          "  outputDir: '.custom-kuizer',",
          "};",
        ].join("\n"),
      );

      const config = await loadConfig(cwd, "./kuizer.config.js");

      expect(config.framework).toBe("nextjs-pages");
      expect(config.outputDir).toBe(join(cwd, ".custom-kuizer"));
      expect(config.allowedExports).toContain("getServerSideProps");
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("rebases relative paths from an explicit config file", async () => {
    const cwd = mkdtempSync(join(tmpdir(), "kuizer-config-base-"));
    const configDir = join(cwd, "config");
    const configPath = join(configDir, "kuizer.config.js");

    try {
      mkdirSync(configDir);

      writeFileSync(
        configPath,
        [
          "module.exports = {",
          "  tsconfig: '../tsconfig.json',",
          "  entrypoints: ['../src/index.ts'],",
          "  outputDir: '../.kuizer-out',",
          "};",
        ].join("\n"),
      );

      const config = await loadConfig(cwd, "./config/kuizer.config.js");

      expect(config.tsconfig).toBe(join(cwd, "tsconfig.json"));
      expect(config.entrypoints).toEqual([join(cwd, "src/index.ts")]);
      expect(config.outputDir).toBe(join(cwd, ".kuizer-out"));
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });
});
