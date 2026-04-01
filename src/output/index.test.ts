import { describe, it, expect } from "bun:test";
import { colorize, badge, rule, indent } from "./index.js";

describe("output", () => {
  it("rule produces a string of the given length (excluding ANSI codes)", () => {
    const raw = rule(20).replace(/\x1b\[[0-9;]*m/g, "");
    expect(raw).toHaveLength(20);
  });

  it("indent prepends spaces to each line", () => {
    const result = indent("a\nb", 2);
    expect(result).toBe("  a\n  b");
  });

  it("badge returns a string containing the severity label", () => {
    const raw = badge("error").replace(/\x1b\[[0-9;]*m/g, "");
    expect(raw.trim()).toBe("error");
  });

  it("colorize wraps in ANSI codes", () => {
    expect(colorize("x", "error")).toContain("\x1b[");
  });
});
