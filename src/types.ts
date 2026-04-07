import type { Project } from 'ts-morph'

export type Severity = 'error' | 'warn' | 'info'

export type Framework = 'nextjs-app' | 'nextjs-pages' | 'vite' | 'none'

export type ExportKind =
	| 'function'
	| 'const'
	| 'class'
	| 'type'
	| 'interface'
	| 'enum'
	| 'default'
	| 'other'

/** Extra machine-readable data attached to a finding when a detector has it. */
type FindingContext = Readonly<Record<string, unknown>>

export type DetectorResult = readonly Finding[]

export type AnalysisBoundary = Readonly<{
	file: string
	line: number
	col: number
	kind: 'dynamic-import'
	message: string
}>

/** A single detector result, immutable once produced. */
export type Finding = Readonly<{
	detectorId: string
	severity: Severity
	file: string
	line: number
	col: number
	message: string
	symbol?: string
	kind?: ExportKind
	fixable: boolean
	context?: FindingContext
}>

type DetectorOpts = Readonly<{
	enabled?: boolean
	severity?: Severity
	ignore?: readonly string[]
}>

type UnusedExportDetectorKey =
	| 'unusedExports'
	| 'unusedImports'
	| 'unusedFunctions'
	| 'unusedTypes'
	| 'unusedClasses'
	| 'unusedComponents'
	| 'unusedConstants'
	| 'unusedEnums'
	| 'unusedDefaults'
	| 'unusedOthers'

type Ignore = Readonly<{
	files: readonly string[]
	symbols: readonly string[]
	paths: readonly string[]
}>

type Detectors = Readonly<
	{
		unusedFiles?: DetectorOpts
	} & Partial<Record<UnusedExportDetectorKey, DetectorOpts>>
>

/** Fully resolved config — all fields present, no optionals at top level. */
export type Config = Readonly<{
	tsconfig: string
	entrypoints: readonly string[]
	framework: Framework
	countTestsAsConsumers: boolean
	allowedExports: readonly string[]
	ignore: Ignore
	detectors: Detectors
	outputDir: string
	dryRunByDefault: boolean
}>

/** Shared analysis context produced by the engine, consumed by detectors. */
export type Ctx = Readonly<{
	project: Project
	reachable: ReadonlySet<string>
	all: ReadonlySet<string>
	analysisBoundaries: readonly AnalysisBoundary[]
}>
