import type { Detector } from "./types.js";
import { detect as unusedFiles } from "./unused-files.js";
import { detect as unusedExports } from "./unused-exports.js";

export const registry = {
  unusedFiles,
  unusedExports,
} as const satisfies Record<string, Detector>;
