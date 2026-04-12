import type { Config } from '../types.js'

export type DetectFlags = {
	config: string | undefined
	tsconfig: string | undefined
	only: string | undefined
	skip: string | undefined
	reporter: string | undefined
	json: boolean | undefined
	filter: string | undefined
	excludePattern: string | undefined
	interactive: boolean | undefined
	includeUi: boolean | undefined
}

export type FixFlags = {
	config: string | undefined
	tsconfig: string | undefined
	only: string | undefined
	skip: string | undefined
	dryRun: boolean | undefined
	write: boolean | undefined
	noBackup: boolean | undefined
	includeUi: boolean | undefined
}

export type RestoreFlags = {
	config: string | undefined
	tsconfig: string | undefined
	snapshot: string | undefined
}

export type InitFlags = {
	global: boolean | undefined
	project: boolean | undefined
}

export type CompletionFlags = {
	shell: string | undefined
}

export type RevertFlags = {
	snapshot: string | undefined
}

export type FlagRecord = Record<string, unknown>

export type DetectorId =
	| 'unusedFiles'
	| 'unusedImports'
	| 'unusedFunctions'
	| 'unusedTypes'
	| 'unusedClasses'
	| 'unusedComponents'
	| 'unusedConstants'
	| 'unusedEnums'
	| 'unusedDefaults'
	| 'unusedOthers'

export type ReporterName = 'terminal' | 'json'

export type FilterSelection = Readonly<{
	detectorIds: ReadonlySet<DetectorId>
	kinds: readonly string[]
	includeAll: boolean
}>

export const SPLIT_EXPORT_DETECTOR_IDS = [
	'unusedFunctions',
	'unusedTypes',
	'unusedClasses',
	'unusedComponents',
	'unusedConstants',
	'unusedEnums',
	'unusedDefaults',
	'unusedOthers',
] as const satisfies readonly DetectorId[]

export const DEFAULT_DETECTOR_IDS = [
	'unusedFiles',
	...SPLIT_EXPORT_DETECTOR_IDS,
] as const satisfies readonly DetectorId[]

export const DEFAULT_DETECTOR_ID_SET = new Set<DetectorId>(DEFAULT_DETECTOR_IDS)

export const DETECTOR_ALIAS_MAP: Readonly<Record<string, readonly DetectorId[]>> = {
	files: ['unusedFiles'],
	exports: SPLIT_EXPORT_DETECTOR_IDS,
	unusedExports: SPLIT_EXPORT_DETECTOR_IDS,
	imports: ['unusedImports'],
	components: ['unusedComponents'],
}

export const DETECT_FLAGS = new Set([
	'-c',
	'--config',
	'config',
	'-o',
	'--only',
	'only',
	'-s',
	'--skip',
	'skip',
	'-r',
	'--reporter',
	'--report',
	'report',
	'reporter',
	'-f',
	'--filter',
	'filter',
	'--tsconfig',
	'tsconfig',
	'--exclude-pattern',
	'--interactive',
	'--include-ui',
	'--json',
	'json',
])

export const FIX_FLAGS = new Set([
	'--dry',
	'--dr',
	'--dry-run',
	'dry',
	'-w',
	'--write',
	'write',
	'--no-backup',
	'no-backup',
	'--include-ui',
	'--dryRun',
	'noBackup',
])

export const FLAG_ALIASES: Readonly<Record<string, string>> = {
	'--dry': '--dryRun',
	'--dr': '--dryRun',
	'-c': '--config',
	'-o': '--only',
	'-s': '--skip',
	'-r': '--reporter',
	'-f': '--filter',
	'-w': '--write',
	'--write': '--write',
	'--no-backup': '--noBackup',
	'--tsconfig': '--tsconfig',
	'--config': '--config',
	'--only': '--only',
	'--skip': '--skip',
	'--report': '--reporter',
	'--filter': '--filter',
	'--include-ui': '--includeUi',
}

export const FLAG_TO_COMMAND: Readonly<Record<string, string>> = {
	'--detect': 'detect',
	'--fix': 'fix',
	'--restore': 'restore',
	'--init': 'init',
	'--revert': 'revert',
	version: 'version',
	'--version': 'version',
	v: 'version',
	'-v': 'version',
}

const DETECT_FILTER_KIND_MAP: Readonly<Record<string, string>> = {
	functions: 'function',
	vars: 'const',
	variables: 'const',
	classes: 'class',
	types: 'type',
	interfaces: 'interface',
	enums: 'enum',
	defaults: 'default',
	others: 'other',
	all: 'all',
}

export function resolveReporter(flags: DetectFlags): ReporterName {
	if (flags.json) return 'json'

	const reporter = flags.reporter ?? 'terminal'
	return reporter as ReporterName
}

export function parseCsv(input?: string): string[] {
	if (!input) return []

	return input
		.split(',')
		.map((part) => part.trim())
		.filter(Boolean)
}

export function resolveDetectorToken(
	token: string,
	available: ReadonlySet<DetectorId>
): readonly DetectorId[] | undefined {
	if (available.has(token as DetectorId)) {
		return [token as DetectorId]
	}

	const alias = DETECTOR_ALIAS_MAP[token.toLowerCase()]
	if (alias) {
		const expanded = alias.filter((id) => available.has(id))
		if (expanded.length > 0) {
			return expanded
		}
	}

	return undefined
}

export function normalizeArgv(argv: readonly string[]): string[] {
	let processedArgv = [...argv]

	if (processedArgv.length > 0) {
		const firstArg = processedArgv[0]
		if (firstArg && DETECT_FLAGS.has(firstArg)) {
			if (firstArg === 'json') {
				processedArgv = ['detect', '--json', ...processedArgv.slice(1)]
			} else {
				processedArgv = ['detect', ...processedArgv]
			}
		} else if (firstArg && FIX_FLAGS.has(firstArg)) {
			processedArgv = ['fix', ...processedArgv]
		} else if (firstArg && firstArg in FLAG_TO_COMMAND) {
			const mappedCommand = FLAG_TO_COMMAND[firstArg]
			if (mappedCommand) {
				processedArgv[0] = mappedCommand
			}
		}
	}

	return processedArgv.map((arg, index) => {
		if (arg === 'json' && index > 0) {
			const prevArg = processedArgv[index - 1]
			if (prevArg === 'detect' || prevArg === 'd' || prevArg === 'fix' || prevArg === 'f') {
				return '--json'
			}
		}

		return arg === '--help' ? arg : FLAG_ALIASES[arg] || arg
	})
}

export function collectFlagValidationErrors(command: string, flags: FlagRecord): string[] {
	const errors: string[] = []
	const requireNonEmpty = (value: unknown, message: string) => {
		if (value === '') {
			errors.push(message)
		}
	}

	switch (command) {
		case 'detect':
			requireNonEmpty(flags.config, 'Config path cannot be empty. Use: kuizer -c <path>')
			requireNonEmpty(
				flags.tsconfig,
				'Tsconfig path cannot be empty. Use: kuizer --tsconfig <path>'
			)
			requireNonEmpty(flags.reporter, 'Reporter cannot be empty. Use: kuizer -r <format>')
			requireNonEmpty(flags.only, 'Only flag cannot be empty. Use: kuizer -o <detectors>')
			requireNonEmpty(flags.skip, 'Skip flag cannot be empty. Use: kuizer -s <detectors>')
			requireNonEmpty(flags.filter, 'Filter cannot be empty. Use: kuizer -f <types>')
			break
		case 'fix':
			requireNonEmpty(flags.config, 'Config path cannot be empty. Use: kuizer -c <path>')
			requireNonEmpty(
				flags.tsconfig,
				'Tsconfig path cannot be empty. Use: kuizer --tsconfig <path>'
			)
			requireNonEmpty(flags.only, 'Only flag cannot be empty. Use: kuizer -o <detectors>')
			requireNonEmpty(flags.skip, 'Skip flag cannot be empty. Use: kuizer -s <detectors>')
			if (flags.write === true && flags.dryRun === true) {
				errors.push('Cannot combine --write with --dryRun/--dry. Choose one mode.')
			}
			break
		case 'restore':
			requireNonEmpty(flags.config, 'Config path cannot be empty. Use: kuizer -c <path>')
			requireNonEmpty(
				flags.tsconfig,
				'Tsconfig path cannot be empty. Use: kuizer --tsconfig <path>'
			)
			requireNonEmpty(
				flags.snapshot,
				'Snapshot path cannot be empty. Use: kuizer restore --snapshot <path|id>'
			)
			break
		case 'completion':
			requireNonEmpty(
				flags.shell,
				'Shell cannot be empty. Use: kuizer completion --shell <bash|zsh|fish>'
			)
			break
	}

	return errors
}

const ALL_DETECTOR_IDS: readonly DetectorId[] = [
	'unusedFiles',
	'unusedImports',
	'unusedFunctions',
	'unusedTypes',
	'unusedClasses',
	'unusedComponents',
	'unusedConstants',
	'unusedEnums',
	'unusedDefaults',
	'unusedOthers',
]

export function resolveDetectorIds(config: Config, flags: { only?: string; skip?: string }): DetectorId[] {
	const availableSet = new Set(ALL_DETECTOR_IDS)
	const only = flags.only ? parseCsv(flags.only) : []
	const skip = new Set<DetectorId>()
	const enabled = [
		...DEFAULT_DETECTOR_IDS.filter(
			(id) => availableSet.has(id) && (config.detectors as any)[id]?.enabled !== false
		),
		...ALL_DETECTOR_IDS.filter(
			(id) => !DEFAULT_DETECTOR_ID_SET.has(id) && (config.detectors as any)[id]?.enabled === true
		),
	]
	const unknown: string[] = []
	const normalizedOnly: DetectorId[] = []

	for (const detectorId of only) {
		const normalized = resolveDetectorToken(detectorId, availableSet)
		if (!normalized) {
			unknown.push(detectorId)
			continue
		}

		normalizedOnly.push(...normalized)
	}

	for (const detectorId of flags.skip ? parseCsv(flags.skip) : []) {
		const normalized = resolveDetectorToken(detectorId, availableSet)
		if (!normalized) {
			unknown.push(detectorId)
			continue
		}

		for (const id of normalized) {
			skip.add(id)
		}
	}

	if (unknown.length > 0) {
		throw new Error(`Unknown detector '${unknown[0]}'`)
	}

	return [...new Set(only.length > 0 ? normalizedOnly : enabled)].filter(
		(id): id is DetectorId => !skip.has(id)
	)
}

export function resolveFilterSelection(filter?: string): FilterSelection {
	const availableSet = new Set(ALL_DETECTOR_IDS)
	const detectorIds = new Set<DetectorId>()
	const kinds = new Set<string>()
	let includeAll = false

	for (const token of parseCsv(filter)) {
		const normalized = token.toLowerCase()
		if (normalized === 'all') {
			includeAll = true
			continue
		}

		const detectorId = resolveDetectorToken(token, availableSet)
		if (detectorId) {
			for (const id of detectorId) {
				detectorIds.add(id)
			}
			continue
		}

		const kind = DETECT_FILTER_KIND_MAP[normalized]
		kinds.add(kind ?? token)
	}

	return {
		detectorIds,
		kinds: [...kinds],
		includeAll,
	}
}

export function applyFindingFilters<T extends { detectorId: string; kind?: string }>(
	findings: readonly T[],
	selection: FilterSelection
): T[] {
	if (
		selection.includeAll ||
		(selection.detectorIds.size === 0 && selection.kinds.length === 0)
	) {
		return [...findings]
	}

	const filterKinds = new Set(selection.kinds)

	return findings.filter((finding) => {
		if (selection.detectorIds.has(finding.detectorId as DetectorId)) {
			return true
		}

		if (filterKinds.size === 0) {
			return false
		}

		return finding.kind !== undefined && filterKinds.has(finding.kind)
	})
}

export function compileExcludePattern(pattern?: string): RegExp | undefined {
	if (!pattern) {
		return undefined
	}

	try {
		return new RegExp(pattern)
	} catch {
		throw new Error(`Invalid regex pattern: ${pattern}`)
	}
}

export function applyExcludePattern<T extends { symbol?: string }>(findings: readonly T[], pattern?: RegExp): T[] {
	if (!pattern) {
		return [...findings]
	}

	return findings.filter((finding) => !pattern.test(finding.symbol || ''))
}
