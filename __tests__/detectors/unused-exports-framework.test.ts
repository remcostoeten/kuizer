import { describe, expect, it } from "bun:test";
import { detect } from "../../src/detectors/unused-exports.js";
import { preset } from "../../src/config/presets.js";
import { makeConfig, makeCtx } from "./fixture.js";

describe("unusedExports framework handling", () => {
  it("does not flag nextjs-app default exports or generateMetadata in page entries", () => {
    const pagePath = "/repo/app/blog/page.tsx";
    const ctx = makeCtx(
      {
        [pagePath]: [
          "export default function Page() { return null }",
          "export function generateMetadata() { return {} }",
          "export const helper = 1",
        ].join("\n"),
      },
      {
        [pagePath]: [
          "export default function Page() { return null }",
          "export function generateMetadata() { return {} }",
          "export const helper = 1",
        ].join("\n"),
      },
    );
    const config = makeConfig({
      framework: "nextjs-app",
      ...preset("nextjs-app"),
    });

    const findings = detect(ctx, config);

    expect(findings).toHaveLength(1);
    expect(findings[0]?.symbol).toBe("helper");
  });

  it("does not flag nextjs-pages default exports for page routes", () => {
    const pagePath = "/repo/pages/about.tsx";
    const ctx = makeCtx(
      {
        [pagePath]: [
          "export default function About() { return null }",
          "export const helper = 1",
        ].join("\n"),
      },
      {
        [pagePath]: [
          "export default function About() { return null }",
          "export const helper = 1",
        ].join("\n"),
      },
    );
    const config = makeConfig({
      framework: "nextjs-pages",
      ...preset("nextjs-pages"),
    });

    const findings = detect(ctx, config);

    expect(findings).toHaveLength(1);
    expect(findings[0]?.symbol).toBe("helper");
  });

  it("does not flag nextjs-app viewport in page entries", () => {
    const pagePath = "/repo/app/blog/page.tsx";
    const ctx = makeCtx(
      {
        [pagePath]: [
          "export default function Page() { return null }",
          "export const viewport = { width: 'device-width' }",
          "export const helper = 1",
        ].join("\n"),
      },
      {
        [pagePath]: [
          "export default function Page() { return null }",
          "export const viewport = { width: 'device-width' }",
          "export const helper = 1",
        ].join("\n"),
      },
    );
    const config = makeConfig({
      framework: "nextjs-app",
      ...preset("nextjs-app"),
    });

    const findings = detect(ctx, config);

    expect(findings).toHaveLength(1);
    expect(findings[0]?.symbol).toBe("helper");
  });

  it("does not flag playwright.config.ts and next.config.ts default exports", () => {
    const ctx = makeCtx(
      {
        "/repo/playwright.config.ts": "export default {}",
        "/repo/next.config.ts": "export default {}",
      },
      {
        "/repo/playwright.config.ts": "export default {}",
        "/repo/next.config.ts": "export default {}",
      },
    );
    const config = makeConfig({ framework: "none" });

    const findings = detect(ctx, config);
    expect(findings).toHaveLength(0);
  });
});

