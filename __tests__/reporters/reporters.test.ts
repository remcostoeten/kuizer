import { afterEach, describe, expect, it, spyOn } from "bun:test";
import { json } from "../../src/reporters/json.js";
import { terminal } from "../../src/reporters/terminal.js";
import type { Report } from "../../src/reporters/types.js";

const baseReport: Report = {
  findings: [
    {
      detectorId: "unusedFiles",
      severity: "warn",
      file: "/src/dead.ts",
      line: 1,
      col: 1,
      message: "File is unreachable from any entry point",
      fixable: false,
    },
  ],
  durationMs: 42,
  analysisBoundaries: [
    {
      file: "/src/index.ts",
      line: 2,
      col: 7,
      kind: "dynamic-import",
      message: "Non-literal dynamic import cannot be resolved statically",
    },
  ],
};

afterEach(() => {
  spyOn(process.stdout, "write").mockRestore();
});

describe("reporters", () => {
  it("json includes analysis boundaries", () => {
    let output = "";
    spyOn(process.stdout, "write").mockImplementation((chunk: string | Uint8Array) => {
      output += String(chunk);
      return true;
    });

    json(baseReport);

    const parsed = JSON.parse(output);
    expect(parsed.meta.analysisBoundaryCount).toBe(1);
    expect(parsed.analysisBoundaries).toHaveLength(1);
    expect(parsed.analysisBoundaries[0].kind).toBe("dynamic-import");
  });

  it("terminal prints analysis boundaries when present", () => {
    let output = "";
    spyOn(process.stdout, "write").mockImplementation((chunk: string | Uint8Array) => {
      output += String(chunk);
      return true;
    });

    terminal(baseReport);

    expect(output).toContain("Analysis boundaries");
    expect(output).toContain("dynamic-import");
    expect(output).toContain("boundaries");
  });
});
