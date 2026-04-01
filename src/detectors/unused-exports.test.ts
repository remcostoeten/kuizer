import { describe, it, expect } from "bun:test";
import { detect } from "./unused-exports.js";
import { makeCtx, makeConfig } from "./fixture.js";

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
});
