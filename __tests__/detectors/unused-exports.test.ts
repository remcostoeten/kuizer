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
    expect(findings[0]?.context).toEqual({
      consumerCount: 0,
      framework: "none",
      analysisBoundaryCount: 0,
      hasDynamicImportBoundary: false,
      internalOnly: false,
      testOnly: false,
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
      internalOnly: false,
      testOnly: false,
    });
  });

  describe("Internal/Same-file consumers", () => {
    it("recognizes same-file usage as internal-only", () => {
      const ctx = makeCtx(
        {
          "/src/utils.ts": `export function usedInternal() {};\nusedInternal();`
        },
        {
          "/src/utils.ts": `export function usedInternal() {};\nusedInternal();`
        }
      );
      const findings = detect(ctx, makeConfig());
      expect(findings).toHaveLength(1);
      expect(findings[0]?.context?.internalOnly).toBe(true);
      expect(findings[0]?.message).toContain("used internally");
    });
  });

  describe("Type-only consumers", () => {
    it("recognizes import { type X } as a real consumer", () => {
      const ctx = makeCtx(
        {
          "/src/types.ts": `export type Foo = string;\nexport type Bar = number;`,
          "/src/main.ts": `import { type Foo, Bar } from './types.js';\nlet a: Foo; let b: Bar;`
        },
        {
          "/src/types.ts": `export type Foo = string;\nexport type Bar = number;`,
          "/src/main.ts": `import { type Foo, Bar } from './types.js';\nlet a: Foo; let b: Bar;`
        }
      );
      const findings = detect(ctx, makeConfig());
      expect(findings).toHaveLength(0);
    });
  });

  describe("Dynamic imports", () => {
    it("recognizes .then((mod) => mod.X) patterns", () => {
      const ctx = makeCtx(
        {
          "/src/lazy.ts": `export function feature() {}`,
          "/src/main.ts": `import('./lazy.js').then((mod) => mod.feature)`
        },
        {
          "/src/lazy.ts": `export function feature() {}`,
          "/src/main.ts": `import('./lazy.js').then((mod) => mod.feature)`
        }
      );
      const findings = detect(ctx, makeConfig());
      expect(findings).toHaveLength(0);
    });

    it("recognizes .then((mod) => ({ default: mod.X })) patterns", () => {
      const ctx = makeCtx(
        {
          "/src/lazy.ts": `export function feature() {}`,
          "/src/main.ts": `import('./lazy.js').then((mod) => ({ default: mod.feature }))`
        },
        {
          "/src/lazy.ts": `export function feature() {}`,
          "/src/main.ts": `import('./lazy.js').then((mod) => ({ default: mod.feature }))`
        }
      );
      const findings = detect(ctx, makeConfig());
      expect(findings).toHaveLength(0);
    });
  });

  describe("Test consumers behavior", () => {
    it("reports test-only usage if countTestsAsConsumers is false", () => {
      const context = makeCtx(
        {
          "/src/utils.ts": `export function testHelper() {}\nexport function used() {}`,
          "/src/main.ts": `import { used } from './utils.js'; used();`,
          "/__tests__/utils.test.ts": `import { testHelper } from '../src/utils.js';\ntestHelper();`
        },
        {
          "/src/utils.ts": `export function testHelper() {}\nexport function used() {}`,
          "/src/main.ts": `import { used } from './utils.js'; used();`,
        }
      );
      // add testReachable to the mocked context
      context.testReachable = new Set(["/__tests__/utils.test.ts"]);
      
      const config = makeConfig({ countTestsAsConsumers: false });
      const findings = detect(context, config);
      expect(findings).toHaveLength(1);
      expect(findings[0]?.context?.testOnly).toBe(true);
      expect(findings[0]?.message).toContain("test-only");
    });
  });

  describe("Dynamic imports with non-literals", () => {
    it("does not crash when encountering non-literal dynamic imports", () => {
      const ctx = makeCtx(
        {
          "/src/main.ts": `const name = 'lazy'; import(name).then((mod) => mod.X)`
        },
        {
          "/src/main.ts": `const name = 'lazy'; import(name).then((mod) => mod.X)`
        }
      );
      // This should not throw 'Expected the module specifier to be a string literal.'
      const findings = detect(ctx, makeConfig());
      expect(findings).toHaveLength(0);
    });
  });
});
