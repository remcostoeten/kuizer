import { describe, expect, it } from "bun:test";
import { preset } from "../../src/config/presets.js";

describe("preset", () => {
  it("covers the documented nextjs-app framework exports", () => {
    const nextApp = preset("nextjs-app");

    expect(nextApp.allowedExports).toEqual([
      "generateMetadata",
      "generateStaticParams",
      "generateViewport",
      "revalidate",
      "dynamic",
      "runtime",
      "fetchCache",
      "preferredRegion",
      "maxDuration",
    ]);
  });

  it("does not globally suppress all nextjs-pages default exports", () => {
    const nextPages = preset("nextjs-pages");

    expect(nextPages.allowedExports).not.toContain("default");
  });

  it("includes Next.js special entrypoints instead of suppressing them", () => {
    const nextApp = preset("nextjs-app");

    expect(nextApp.entrypoints).toContain("src/proxy.ts");
    expect(nextApp.entrypoints).toContain("src/app/**/manifest.ts");
    expect(nextApp.entrypoints).toContain("src/app/**/robots.ts");
    expect(nextApp.entrypoints).toContain("src/app/**/sitemap.ts");
    expect(nextApp.entrypoints).toContain("src/app/**/opengraph-image.tsx");
    expect(nextApp.entrypoints).toContain("src/app/**/twitter-image.tsx");
    expect(nextApp.ignore.files).not.toContain("**/route.ts");
  });

  it("ignores common Next.js toolchain files by default", () => {
    const nextApp = preset("nextjs-app");

    expect(nextApp.ignore.files).toContain("next-env.d.ts");
    expect(nextApp.ignore.files).toContain("next.config.ts");
    expect(nextApp.ignore.files).toContain("drizzle.config.ts");
    expect(nextApp.ignore.files).toContain("vitest.config.ts");
    expect(nextApp.ignore.files).toContain("**/test/setup.ts");
  });
});
