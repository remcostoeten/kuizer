import type { Config, Ctx, Finding } from '../types.js'

type SafetyTier = 'safe' | 'cautious' | 'destructive'

type DeleteFileEdit = Readonly<{
	kind: 'delete-file'
	file: string
	description: string
}>

type RemoveExportKeywordEdit = Readonly<{
	kind: 'remove-export-keyword'
	file: string
	exportName: string
	startLine: number
	endLine: number
	description: string
}>

type DeleteDeclarationEdit = Readonly<{
	kind: 'delete-declaration'
	file: string
	exportName: string
	startLine: number
	endLine: number
	description: string
}>

export type PlannedEdit = DeleteFileEdit | RemoveExportKeywordEdit | DeleteDeclarationEdit

export type FixPlan = Readonly<{
	supported: true
	finding: Finding
	edits: readonly PlannedEdit[]
	safety: SafetyTier
	reason: string
}>

export type UnsupportedFix = Readonly<{
	supported: false
	finding: Finding
	reason: string
}>

type PlannerResult = FixPlan | UnsupportedFix | null

export type FixPlanner = (finding: Finding, ctx: Ctx, config: Config) => PlannerResult

export type FixPlanningResult = Readonly<{
	supported: readonly FixPlan[]
	unsupported: readonly UnsupportedFix[]
}>

export type AppliedFixResult = Readonly<{
	applied: readonly FixPlan[]
	skipped: readonly UnsupportedFix[]
	snapshotPath?: string
}>
