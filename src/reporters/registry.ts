import { report as json } from "./json.js";
import { report as terminal } from "./terminal.js";
import type { Reporter } from "./types.js";


export const registry = {
  terminal,
  json,
} as const satisfies Record<string, Reporter>