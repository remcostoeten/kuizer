import { describe, it, expect } from "bun:test";
import { loadConfig } from "./index.js";

describe("loadConfig", () => {
  it("returns fully resolved config with defaults when no file exists", async () => {
    const config = await loadConfig("/tmp");

    expect(config.framework).toBe("none");
    expect(config.tsconfig).toBe("./tsconfig.json");
    expect(config.outputDir).toBe(".kuizer");
    expect(config.entrypoints).toBeArray();
  });
});
