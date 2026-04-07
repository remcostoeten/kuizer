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
});
