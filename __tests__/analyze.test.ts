import fs from "fs";
import os from "os";
import path from "path";
import { describe, expect, test } from "vitest";
import { analyze, type TSet } from "../src/index.js";

function mkTmp() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "kuizer-"));
  const src = path.join(dir, "src");
  fs.mkdirSync(src, { recursive: true });

  fs.writeFileSync(
    path.join(dir, "tsconfig.json"),
    JSON.stringify(
      {
        compilerOptions: {
          target: "ES2022",
          module: "ESNext",
          moduleResolution: "Bundler",
          strict: true,
          baseUrl: ".",
          paths: { "@/*": ["src/*"] }
        },
        include: ["src/**/*.ts"]
      },
      null,
      2
    )
  );

  return { dir, src };
}

function mkSet(dir: string, src: string): TSet {
  return {
    root: dir,
    tsCfg: path.join(dir, "tsconfig.json"),
    src,
    bars: ["index.ts", "index.tsx"]
  };
}

describe("analyze", () => {
  test("returns empty on empty project for now", () => {
    const { dir, src } = mkTmp();
    const set = mkSet(dir, src);
    const res = analyze(set);
    expect(res).toEqual([]);
  });

  test.skip("case: exported type unused anywhere, should be rm-decl", () => {});
  test.skip("case: exported type used only in same file, should be unexport", () => {});
  test.skip("case: exported type used in other file, should be keep", () => {});
  test.skip("case: barrel-only usage, should be rm-decl or unexport", () => {});
});
