import { existsSync, readFileSync } from "node:fs";
import { basename } from "node:path";
import type { FixPlan } from "./types.js";

export function renderPlanDiff(plan: FixPlan): string {
  return plan.edits.map((edit) => {
    if (edit.kind === "delete-file") {
      return renderDeleteFileDiff(edit.file);
    }

    return `Unsupported edit preview for ${edit.kind}`;
  }).join("\n");
}

function renderDeleteFileDiff(file: string): string {
  const header = [`--- ${file}`, `+++ (deleted)`, `@@ delete-file @@`];
  if (!existsSync(file)) {
    return [...header, `(missing) ${basename(file)}`].join("\n");
  }

  const contents = readFileSync(file, "utf8")
    .split("\n")
    .map((line) => `- ${line}`)
    .join("\n");

  return [...header, contents].join("\n");
}
