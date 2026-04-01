import type { Config } from "../types.js";

export const defaults = {
  tsconfig: "./tsconfig.json",
  entrypoints: [],
  framework: "none",
  countTestsAsConsumers: false,
  allowedExports: [],
  ignore: {
    files: [],
    symbols: [],
    paths: [],
  },
  detectors: {},
  outputDir: ".kuizer",
} as const satisfies Config;
