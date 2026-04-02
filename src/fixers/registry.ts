import type { FixPlanner } from "./types.js";
import { planUnusedFilesFix } from "./planners/unused-files.js";

export const registry = {
  unusedFiles: planUnusedFilesFix,
} as const satisfies Record<string, FixPlanner>;
