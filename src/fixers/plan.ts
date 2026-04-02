import type { Config, Ctx, Finding } from "../types.js";
import { registry } from "./registry.js";
import type { FixPlanningResult, UnsupportedFix } from "./types.js";

export function planFixes(
  findings: readonly Finding[],
  ctx: Ctx,
  config: Config,
): FixPlanningResult {
  const supported: Array<FixPlanningResult["supported"][number]> = [];
  const unsupported: UnsupportedFix[] = [];

  for (const finding of findings) {
    const planner = registry[finding.detectorId as keyof typeof registry];
    if (!planner) {
      unsupported.push({
        supported: false,
        finding,
        reason: `No fixer planner is registered for detector '${finding.detectorId}'`,
      });
      continue;
    }

    const result = planner(finding, ctx, config);
    if (!result) {
      unsupported.push({
        supported: false,
        finding,
        reason: `Detector '${finding.detectorId}' does not support fixer planning for this finding`,
      });
      continue;
    }

    if (result.supported) {
      supported.push(result);
      continue;
    }

    unsupported.push(result);
  }

  return { supported, unsupported };
}
