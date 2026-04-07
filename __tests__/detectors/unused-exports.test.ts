import { describe, expect, it } from "bun:test";
import { detect } from "../../src/detectors/unused-exports.js";
import { detect as unusedImports } from "../../src/detectors/unused-imports.js";
import { makeConfig, makeCtx } from "./fixture.js";

const consumer = (path: string) => `import { used } from '${path}';\nused()`;

describe("unusedExports", () => {
  it("flags an export with no consumers", () => {
    const ctx = makeCtx(
      { "/src/utils.ts": `export function dead() {}` },
      { "/src/utils.ts": `export function dead() {}` },
    );

    const findings = detect(ctx, makeConfig());
    expect(findings).toHaveLength(1);
    expect(findings[0]?.symbol).toBe("dead");
    expect(findings[0]?.line).toBe(1);
    expect(findings[0]?.col).toBe(1);
    expect(findings[0]?.context).toEqual({
      consumerCount: 0,
      framework: "none",
      analysisBoundaryCount: 0,
      hasDynamicImportBoundary: false,
    });
  });

  it("does not flag an export that is consumed", () => {
    const ctx = makeCtx(
      {
        "/src/utils.ts": `export function used() {}`,
        "/src/main.ts": consumer("/src/utils"),
      },
      {
        "/src/utils.ts": `export function used() {}`,
        "/src/main.ts": consumer("/src/utils"),
      },
    );

    expect(detect(ctx, makeConfig())).toHaveLength(0);
  });

  it("flags a dead export even when the only reference is an unused import", () => {
    const ctx = makeCtx(
      {
        "/src/dead.ts": `export function dead() {}`,
        "/src/main.ts": `import { dead } from '/src/dead'\nconsole.log('x')`,
      },
      {
        "/src/dead.ts": `export function dead() {}`,
        "/src/main.ts": `import { dead } from '/src/dead'\nconsole.log('x')`,
      },
    );

    expect(unusedImports(ctx, makeConfig())).toHaveLength(1);
    expect(detect(ctx, makeConfig()).map((finding) => finding.symbol)).toEqual(["dead"]);
  });

  it("treats direct namespace property access as a consumer", () => {
    const ctx = makeCtx(
      {
        "/src/utils.ts": `export const value = 1`,
        "/src/main.ts": `import * as Utils from '/src/utils';\nconsole.log(Utils.value)`,
      },
      {
        "/src/utils.ts": `export const value = 1`,
        "/src/main.ts": `import * as Utils from '/src/utils';\nconsole.log(Utils.value)`,
      },
    );

    expect(detect(ctx, makeConfig())).toHaveLength(0);
  });

  it("ignores consumers from unreachable files", () => {
    const ctx = makeCtx(
      {
        "/src/utils.ts": `export function dead() {}`,
        "/src/dead-consumer.ts": `import { dead } from '/src/utils';\ndead()`,
      },
      {
        "/src/utils.ts": `export function dead() {}`,
      },
    );

    const findings = detect(ctx, makeConfig());
    expect(findings).toHaveLength(1);
    expect(findings[0]?.symbol).toBe("dead");
  });

  it("treats namespace property access through barrels as a consumer", () => {
    const ctx = makeCtx(
      {
        "/src/origin.ts": `export const value = 1`,
        "/src/barrel.ts": `export { value } from '/src/origin'`,
        "/src/main.ts": `import * as Barrel from '/src/barrel';\nconsole.log(Barrel.value)`,
      },
      {
        "/src/origin.ts": `export const value = 1`,
        "/src/barrel.ts": `export { value } from '/src/origin'`,
        "/src/main.ts": `import * as Barrel from '/src/barrel';\nconsole.log(Barrel.value)`,
      },
    );

    expect(detect(ctx, makeConfig())).toHaveLength(0);
  });

  it("respects allowedExports", () => {
    const ctx = makeCtx(
      { "/src/page.ts": `export function generateMetadata() {}` },
      { "/src/page.ts": `export function generateMetadata() {}` },
    );
    const config = makeConfig({ allowedExports: ["generateMetadata"] });

    expect(detect(ctx, config)).toHaveLength(0);
  });

  it("respects ignore.symbols", () => {
    const ctx = makeCtx(
      { "/src/utils.ts": `export function dead() {}` },
      { "/src/utils.ts": `export function dead() {}` },
    );
    const config = makeConfig({
      ignore: { symbols: ["dead"], files: [], paths: [] },
    });

    expect(detect(ctx, config)).toHaveLength(0);
  });

  it("uses configured severity", () => {
    const ctx = makeCtx(
      { "/src/utils.ts": `export function dead() {}` },
      { "/src/utils.ts": `export function dead() {}` },
    );
    const config = makeConfig({
      detectors: { unusedExports: { severity: "error" } },
    });

    expect(detect(ctx, config)[0]?.severity).toBe("error");
  });

  it("treats named re-export chains as live", () => {
    const ctx = makeCtx(
      {
        "/src/origin.ts": `export const value = 1`,
        "/src/barrel.ts": `export { value } from '/src/origin'`,
        "/src/main.ts": `import { value } from '/src/barrel';\nconsole.log(value)`,
      },
      {
        "/src/origin.ts": `export const value = 1`,
        "/src/barrel.ts": `export { value } from '/src/origin'`,
        "/src/main.ts": `import { value } from '/src/barrel';\nconsole.log(value)`,
      },
    );

    expect(detect(ctx, makeConfig())).toHaveLength(0);
  });

  it("treats export-star chains as live", () => {
    const ctx = makeCtx(
      {
        "/src/origin.ts": `export const value = 1`,
        "/src/barrel.ts": `export * from '/src/origin'`,
        "/src/main.ts": `import { value } from '/src/barrel';\nconsole.log(value)`,
      },
      {
        "/src/origin.ts": `export const value = 1`,
        "/src/barrel.ts": `export * from '/src/origin'`,
        "/src/main.ts": `import { value } from '/src/barrel';\nconsole.log(value)`,
      },
    );

    expect(detect(ctx, makeConfig())).toHaveLength(0);
  });

  it("treats default export aliases through barrels as live", () => {
    const ctx = makeCtx(
      {
        "/src/origin.ts": `export default function value() { return 1 }`,
        "/src/barrel.ts": `export { default as value } from '/src/origin'`,
        "/src/main.ts": `import { value } from '/src/barrel';\nconsole.log(value())`,
      },
      {
        "/src/origin.ts": `export default function value() { return 1 }`,
        "/src/barrel.ts": `export { default as value } from '/src/origin'`,
        "/src/main.ts": `import { value } from '/src/barrel';\nconsole.log(value())`,
      },
    );

    expect(detect(ctx, makeConfig())).toHaveLength(0);
  });

  it("treats namespace access to default export aliases through barrels as live", () => {
    const ctx = makeCtx(
      {
        "/src/origin.ts": `export default function value() { return 1 }`,
        "/src/barrel.ts": `export { default as value } from '/src/origin'`,
        "/src/main.ts": `import * as Barrel from '/src/barrel';\nconsole.log(Barrel.value())`,
      },
      {
        "/src/origin.ts": `export default function value() { return 1 }`,
        "/src/barrel.ts": `export { default as value } from '/src/origin'`,
        "/src/main.ts": `import * as Barrel from '/src/barrel';\nconsole.log(Barrel.value())`,
      },
    );

    expect(detect(ctx, makeConfig())).toHaveLength(0);
  });

  it("treats local export aliases as consumed when imported by their exported name", () => {
    const ctx = makeCtx(
      {
        "/src/origin.ts": `const foo = 1\nexport { foo as bar }`,
        "/src/main.ts": `import { bar } from '/src/origin';\nconsole.log(bar)`,
      },
      {
        "/src/origin.ts": `const foo = 1\nexport { foo as bar }`,
        "/src/main.ts": `import { bar } from '/src/origin';\nconsole.log(bar)`,
      },
    );

    expect(detect(ctx, makeConfig())).toHaveLength(0);
  });

  it("flags re-exported symbols when the barrel chain has no reachable consumers", () => {
    const ctx = makeCtx(
      {
        "/src/origin.ts": `export const dead = 1`,
        "/src/barrel.ts": `export { dead } from '/src/origin'`,
      },
      {
        "/src/origin.ts": `export const dead = 1`,
        "/src/barrel.ts": `export { dead } from '/src/origin'`,
      },
    );

    const findings = detect(ctx, makeConfig());
    expect(findings).toHaveLength(1);
    expect(findings[0]?.symbol).toBe("dead");
    expect(findings[0]?.file).toBe("/src/origin.ts");
  });

  it("adds uncertainty context when analysis boundaries exist", () => {
    const ctx = {
      ...makeCtx(
        { "/src/utils.ts": `export function dead() {}` },
        { "/src/utils.ts": `export function dead() {}` },
      ),
      analysisBoundaries: [
        {
          file: "/src/index.ts",
          line: 2,
          col: 1,
          kind: "dynamic-import" as const,
          message: "Non-literal dynamic import cannot be resolved statically",
        },
      ],
    };

    const finding = detect(ctx, makeConfig())[0];
    expect(finding?.message).toContain("statically resolved consumers");
    expect(finding?.context).toEqual({
      consumerCount: 0,
      framework: "none",
      analysisBoundaryCount: 1,
      hasDynamicImportBoundary: true,
    });
  });
});
