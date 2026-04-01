import type { Reporter } from "./types.js";
import { report as terminal } from "./terminal.js";
import { report as json } from "./json.js";

export const registry = {
  terminal,
  json,
} as const satisfies Record<string, Reporter>;
