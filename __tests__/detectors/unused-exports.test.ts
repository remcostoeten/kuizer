import { describe, expect, it } from "bun:test";
import { detect } from "../../src/detectors/unused-exports.js";
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
});
