import type { Config, Ctx, Finding } from "../types.js";

export type SafetyTier = "safe" | "cautious" | "destructive";

export type DeleteFileEdit = Readonly<{
  kind: "delete-file";
  file: string;
  description: string;
}>;

export type PlannedEdit = DeleteFileEdit;

export type FixPlan = Readonly<{
  supported: true;
  finding: Finding;
  edits: readonly PlannedEdit[];
  safety: SafetyTier;
  reason: string;
}>;

export type UnsupportedFix = Readonly<{
  supported: false;
  finding: Finding;
  reason: string;
}>;

export type PlannerResult = FixPlan | UnsupportedFix | null;

export type FixPlanner = (
  finding: Finding,
  ctx: Ctx,
  config: Config,
) => PlannerResult;

export type FixPlanningResult = Readonly<{
  supported: readonly FixPlan[];
  unsupported: readonly UnsupportedFix[];
}>;
