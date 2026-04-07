#!/usr/bin/env node

import { existsSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { resolve } from 'node:path'
import pc from 'picocolors'
import { preset } from '../config/presets.js'
import { applyFixPlans } from '../fixers/apply.js'
import { planFixes } from '../fixers/plan.js'
import { reportAppliedFixes, reportFixPlan } from '../fixers/report.js'
import type { Config, Ctx, Finding, Framework } from '../types.js'

import { bootstrap } from '../engine/index.js'
import { createProgress } from '../output/progress.js'

import { registry as detectorRegistry } from '../detectors/registry.js'
import { registry as reporterRegistry } from '../reporters/registry.js'

import { cli, command } from 'cleye'
import { fileURLToPath } from 'node:url'
import { restoreSnapshot } from '../backup/restore.js'
import { loadConfig } from '../config/index.js'
import { getVersion } from '../meta.js'
import { dim, write } from '../output/index.js'
import { printMainHelp, tryHandleHelp } from './help.js'
import { runInteractiveSession } from './interactive.js'

const version = getVersion(import.meta.url)

type DetectorId = keyof typeof detectorRegistry
type ReporterName = keyof typeof reporterRegistry

type DetectFlags = {
	config: string | undefined
	tsconfig: string | undefined
	only: string | undefined
	skip: string | undefined
	reporter: string | undefined
	json: boolean | undefined
	filter: string | undefined
	excludePattern: string | undefined
	interactive: boolean | undefined
}

type FixFlags = {
	config: string | undefined
	tsconfig: string | undefined
	only: string | undefined
	skip: string | undefined
	dryRun: boolean | undefined
	write: boolean | undefined
	noBackup: boolean | undefined
}

type RestoreFlags = {
	config: string | undefined
	tsconfig: string | undefined
	snapshot: string | undefined
}

type InitFlags = {
	global: boolean | undefined
	project: boolean | undefined
}

type RevertFlags = {
	snapshot: string | undefined
}

type DetectorSelectionFlags = Pick<DetectFlags, 'only' | 'skip'>

type FlagRecord = Record<string, unknown>

type FilterSelection = Readonly<{
	detectorIds: ReadonlySet<DetectorId>
	kinds: readonly string[]
	includeAll: boolean
}>

const SPLIT_EXPORT_DETECTOR_IDS = [
	'unusedFunctions',
	'unusedTypes',
	'unusedClasses',
	'unusedComponents',
	'unusedConstants',
	'unusedEnums',
	'unusedDefaults',
	'unusedOthers',
] as const satisfies readonly DetectorId[]

const DEFAULT_DETECTOR_IDS = [
	'unusedFiles',
	...SPLIT_EXPORT_DETECTOR_IDS,
] as const satisfies readonly DetectorId[]
const DEFAULT_DETECTOR_ID_SET = new Set<DetectorId>(DEFAULT_DETECTOR_IDS)

const DETECTOR_ALIAS_MAP: Readonly<Record<string, readonly DetectorId[]>> = {
	files: ['unusedFiles'],
	exports: SPLIT_EXPORT_DETECTOR_IDS,
	unusedExports: SPLIT_EXPORT_DETECTOR_IDS,
	imports: ['unusedImports'],
	components: ['unusedComponents'],
}

const FLAG_TO_COMMAND: Readonly<Record<string, string>> = {
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

const DETECT_FLAGS = new Set([
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
	'--json',
	'json',
])

const FIX_FLAGS = new Set([
	'--dry',
	'--dr',
	'--dry-run',
	'dry',
	'-w',
	'--write',
	'write',
	'--no-backup',
	'no-backup',
	'--dryRun',
	'noBackup',
])

const FLAG_ALIASES: Readonly<Record<string, string>> = {
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
}

class CliError extends Error {
	exitCode: number

	constructor(message: string, exitCode = 1) {
		super(message)
		this.name = 'CliError'
		this.exitCode = exitCode
	}
}

function parseCsv(input?: string): string[] {
	if (!input) return []

	return input
		.split(',')
		.map((part) => part.trim())
		.filter(Boolean)
}

function resolveDetectorToken(
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

export function resolveReporter(flags: DetectFlags): ReporterName {
	if (flags.json) return 'json'

	const reporter = flags.reporter ?? 'terminal'
	if (reporter in reporterRegistry) {
		return reporter as ReporterName
	}

	throw new Error(`Unknown reporter '${reporter}'`)
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
	}

	return errors
}

function resolveDetectorIds(config: Config, flags: DetectorSelectionFlags): DetectorId[] {
	const available = Object.keys(detectorRegistry) as DetectorId[]
	const availableSet = new Set(available)
	const only = parseCsv(flags.only)
	const skip = new Set<DetectorId>()
	const enabled = [
		...DEFAULT_DETECTOR_IDS.filter(
			(id) => availableSet.has(id) && config.detectors[id]?.enabled !== false
		),
		...available.filter(
			(id) => !DEFAULT_DETECTOR_ID_SET.has(id) && config.detectors[id]?.enabled === true
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

	for (const detectorId of parseCsv(flags.skip)) {
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

function exitCode(findings: readonly Finding[]): number {
	return findings.some((finding) => finding.severity === 'error') ? 1 : 0
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

function resolveFilterSelection(filter?: string): FilterSelection {
	const available = new Set(Object.keys(detectorRegistry) as DetectorId[])
	const detectorIds = new Set<DetectorId>()
	const kinds = new Set<string>()
	let includeAll = false

	for (const token of parseCsv(filter)) {
		const normalized = token.toLowerCase()
		if (normalized === 'all') {
			includeAll = true
			continue
		}

		const detectorId = resolveDetectorToken(token, available)
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

function applyFindingFilters(findings: readonly Finding[], selection: FilterSelection): Finding[] {
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

function compileExcludePattern(pattern?: string): RegExp | undefined {
	if (!pattern) {
		return undefined
	}

	try {
		return new RegExp(pattern)
	} catch {
		throw new Error(`Invalid regex pattern: ${pattern}`)
	}
}

function applyExcludePattern(findings: readonly Finding[], pattern?: RegExp): Finding[] {
	if (!pattern) {
		return [...findings]
	}

	return findings.filter((finding) => !pattern.test(finding.symbol || ''))
}

async function runDetect(flags: DetectFlags): Promise<number> {
	const startedAt = Date.now()
	const config = await loadConfig(process.cwd(), flags.config, flags.tsconfig)

	const reporterName = resolveReporter(flags)
	const interactive = flags.interactive === true
	const showProgress = reporterName === 'terminal' && !interactive
	const progress = showProgress ? createProgress({ cwd: process.cwd() }) : undefined
	const detectors = resolveDetectorIds(config, flags)
	const filterSelection = resolveFilterSelection(flags.filter)
	const excludePattern = compileExcludePattern(flags.excludePattern)

	const analyze = async (withProgress: boolean): Promise<{ ctx: Ctx; findings: Finding[] }> => {
		if (withProgress) {
			progress?.update('Loading TypeScript project')
		}

		const ctx = await bootstrap(
			config,
			withProgress
				? {
						onProgress(event) {
							if (!progress) return

							switch (event.phase) {
								case 'load-project':
									progress.update('Loading TypeScript project')
									break
								case 'collect-files':
									progress.update('Collecting source files')
									break
								case 'resolve-entries':
									progress.update(
										'Resolving entrypoints',
										`${event.totalFiles ?? 0} files`
									)
									break
								case 'analyze-file':
									progress.update(
										`Analyzing import graph ${event.completed}/${event.totalFiles}`,
										event.file
									)
									break
								case 'complete':
									progress.update(
										'Analysis ready',
										`${event.totalFiles ?? 0} files`
									)
									break
							}
						},
					}
				: {}
		)

		let findings: Finding[] = []
		for (const [index, id] of detectors.entries()) {
			if (withProgress) {
				progress?.update(`Running detector ${index + 1}/${detectors.length}`, id)
			}
			findings.push(...detectorRegistry[id](ctx, config))
		}

		return { ctx, findings }
	}

	try {
		const initial = await analyze(true)

		if (interactive) {
			const interactiveFindings = applyFindingFilters(
				applyExcludePattern(initial.findings, excludePattern),
				filterSelection
			)
			const remaining = await runInteractiveSession({
				version,
				config,
				cwd: process.cwd(),
				findings: interactiveFindings,
				supportedFixes: planFixes(interactiveFindings, initial.ctx, config).supported,
				initialKinds: filterSelection.kinds,
				initialDetectors: [...filterSelection.detectorIds],
				async refresh() {
					const refreshed = await analyze(false)
					const refreshedFindings = applyFindingFilters(
						applyExcludePattern(refreshed.findings, excludePattern),
						filterSelection
					)
					return {
						findings: refreshedFindings,
						supportedFixes: planFixes(refreshedFindings, refreshed.ctx, config)
							.supported,
					}
				},
			})
			return exitCode(remaining)
		}

		const findings = applyFindingFilters(
			applyExcludePattern(initial.findings, excludePattern),
			filterSelection
		)
		const reporter = reporterRegistry[reporterName]

		reporter({
			findings,
			durationMs: Date.now() - startedAt,
			analysisBoundaries: initial.ctx.analysisBoundaries,
		})
		return exitCode(findings)
	} catch (error) {
		if (error instanceof Error && error.message.includes('File not found')) {
			throw new CliError(error.message, 1)
		}

		throw error
	} finally {
		progress?.stop()
	}
}

async function runFix(flags: FixFlags): Promise<number> {
	const config = await loadConfig(process.cwd(), flags.config, flags.tsconfig)

	try {
		const ctx = await bootstrap(config)
		const detectors = resolveDetectorIds(config, flags)
		const findings = detectors.flatMap((id) => detectorRegistry[id](ctx, config))
		const plans = planFixes(findings, ctx, config)
		const shouldDryRun =
			flags.dryRun === true || (flags.write !== true && config.dryRunByDefault)

		if (shouldDryRun) {
			reportFixPlan(plans, { dryRun: true })
			return 0
		}

		const result = applyFixPlans(plans.supported, plans.unsupported, config, {
			backup: flags.noBackup !== true,
		})
		reportAppliedFixes(result)
		return 0
	} catch (error) {
		if (error instanceof Error && error.message.includes('File not found')) {
			throw new CliError(error.message, 1)
		}

		throw error
	}
}

async function runRestore(flags: RestoreFlags): Promise<number> {
	if (!flags.snapshot) {
		throw new CliError('Missing required --snapshot path')
	}

	const config = await loadConfig(process.cwd(), flags.config, flags.tsconfig)
	const result = restoreSnapshot(flags.snapshot, {
		cwd: process.cwd(),
		outputDir: config.outputDir,
	})
	write(`Restored ${result.restored.length} file${result.restored.length === 1 ? '' : 's'}`)
	write(dim(`Snapshot: ${result.manifestPath}`))
	return 0
}

async function runInit(flags: InitFlags): Promise<number> {
	// Determine config type
	let isGlobal: boolean
	if (flags.global && flags.project) {
		console.error(pc.red('Error: Cannot specify both --global and --project'))
		return 1
	} else if (flags.global) {
		isGlobal = true
	} else if (flags.project) {
		isGlobal = false
	} else {
		// Interactive prompt
		console.log(pc.yellow('🔧 Kuizer Configuration Setup'))
		console.log('')
		console.log('Where do you want to create the configuration?')
		console.log('1) Global (~/.kuizerrc) - applies to all projects')
		console.log('2) Project (./kuizer.config.json) - applies to current project only')
		console.log('')

		// Simple stdin read for choice
		process.stdout.write(pc.cyan('Choose [1/2]: '))
		const choice = await new Promise<string>((resolve) => {
			process.stdin.once('data', (data) => {
				resolve(data.toString().trim())
			})
		})

		isGlobal = choice === '1'
	}

	const configPath = isGlobal
		? resolve(homedir(), '.kuizerrc')
		: resolve(process.cwd(), 'kuizer.config.json')

	// Check if config already exists
	if (existsSync(configPath)) {
		console.log(pc.red(`Configuration already exists at: ${configPath}`))
		console.log(pc.dim('Remove it first or edit it directly.'))
		return 1
	}

	// Interactive configuration
	console.log('')
	console.log(pc.yellow('Configure your preferences:'))
	console.log('')

	// Ask about dry-run default
	process.stdout.write(pc.cyan('Default to dry-run mode? (recommended) [Y/n]: '))
	const dryRunChoice = await new Promise<string>((resolve) => {
		process.stdin.once('data', (data) => {
			resolve(data.toString().trim().toLowerCase())
		})
	})
	const dryRunByDefault = dryRunChoice !== 'n' && dryRunChoice !== 'no'

	// Ask about framework
	console.log('')
	console.log('Framework presets:')
	console.log('1) none      - Generic TypeScript project')
	console.log('2) nextjs-app - Next.js App Router')
	console.log('3) nextjs-pages - Next.js Pages Router')
	console.log('4) vite      - Vite-based project')
	console.log('')

	process.stdout.write(pc.cyan('Choose framework [1-4]: '))
	const frameworkChoice = await new Promise<string>((resolve) => {
		process.stdin.once('data', (data) => {
			resolve(data.toString().trim())
		})
	})

	const frameworkMap: Record<string, Framework> = {
		'1': 'none',
		'2': 'nextjs-app',
		'3': 'nextjs-pages',
		'4': 'vite',
	}
	const framework = frameworkMap[frameworkChoice] || 'none'

	// Create config object
	const config = {
		dryRunByDefault,
		framework,
		...(framework !== 'none' && preset(framework)),
	}

	// Write config file
	writeFileSync(configPath, JSON.stringify(config, null, 2) + '\n')

	const relativePath = isGlobal ? '~/.kuizerrc' : './kuizer.config.json'
	console.log('')
	console.log(pc.green('✅ Configuration created!'))
	console.log(pc.dim(`Location: ${relativePath}`))
	console.log('')

	if (dryRunByDefault) {
		console.log(pc.dim('🛡️  Safe mode enabled: kuizer fix will dry-run by default'))
		console.log(pc.dim('   Use kuizer fix -w to apply fixes'))
	} else {
		console.log(pc.dim('⚡ Write mode enabled: kuizer fix will apply changes by default'))
		console.log(pc.dim('   Use kuizer fix --dry to preview changes'))
	}

	if (framework !== 'none') {
		console.log(pc.dim(`🔧 Framework preset applied: ${framework}`))
	}

	return 0
}

async function runRevert(flags: RevertFlags): Promise<number> {
	if (!flags.snapshot) {
		console.error(pc.red('Error: Missing required --snapshot path or ID'))
		console.error(pc.dim('Usage: kuizer revert --snapshot <path|id>'))
		console.error(pc.dim('Or: kuizer revert <snapshot-id>'))
		return 1
	}

	try {
		const config = await loadConfig(process.cwd())
		const result = restoreSnapshot(flags.snapshot, {
			cwd: process.cwd(),
			outputDir: config.outputDir,
		})

		write(`Reverted ${result.restored.length} file${result.restored.length === 1 ? '' : 's'}`)
		write(dim(`Snapshot: ${result.manifestPath}`))
		return 0
	} catch (error) {
		console.error(
			pc.red(`Failed to revert: ${error instanceof Error ? error.message : 'Unknown error'}`)
		)
		return 1
	}
}

export async function main(argv: string[] = process.argv.slice(2)): Promise<number> {
	try {
		const normalizedArgv = normalizeArgv(argv)

		// Custom help system — intercept before cleye
		if (tryHandleHelp(normalizedArgv, version)) {
			return 0
		}

		const detect = command({
			name: 'detect',
			alias: 'd',
			help: {
				description: 'Analyze your codebase to find unused exports and dead code',
			},
			flags: {
				config: {
					type: String,
					alias: 'c',
					description:
						'Path to kuizer config file (defaults to kuizer.config.json) (alias: --config)',
				},
				tsconfig: {
					type: String,
					description:
						'Path to tsconfig.json (auto-detected if not provided) (alias: --tsconfig)',
				},
				reporter: {
					type: String,
					alias: 'r',
					description: 'Output format: terminal (colored) or json (alias: --report)',
				},
				only: {
					type: String,
					alias: 'o',
					description:
						'Run only specific detectors (e.g., unusedImports, unusedComponents, exports)',
				},
				skip: {
					type: String,
					alias: 's',
					description: 'Skip specific detectors',
				},
				json: {
					type: Boolean,
					description: 'Output results as JSON (shorthand for --reporter json)',
				},
				filter: {
					type: String,
					alias: 'f',
					description:
						'Filter results by kind or detector: functions, classes, unusedImports, unusedComponents, all',
				},
				excludePattern: {
					type: String,
					description: 'Exclude findings matching a regex pattern (e.g., "^_")',
				},
				interactive: {
					type: Boolean,
					description: 'Interactive TUI to browse and filter results with keyboard',
				},
			},
		})

		const fix = command({
			name: 'fix',
			alias: 'f',
			help: {
				description: 'Automatically remove unused exports and dead code from your project',
			},
			flags: {
				config: {
					type: String,
					alias: 'c',
					description:
						'Path to kuizer config file (defaults to kuizer.config.json) (alias: --config)',
				},
				tsconfig: {
					type: String,
					description:
						'Path to tsconfig.json (auto-detected if not provided) (alias: --tsconfig)',
				},
				only: {
					type: String,
					alias: 'o',
					description: 'Plan fixes for specific detectors only',
				},
				skip: {
					type: String,
					alias: 's',
					description: 'Skip specific detectors when planning fixes',
				},
				dryRun: {
					type: Boolean,
					description:
						'Show what would be fixed without writing any changes (aliases: --dry, --dr)',
				},
				write: {
					type: Boolean,
					alias: 'w',
					description:
						'Apply the fixes to your files (creates a backup by default) (alias: --write)',
				},
				noBackup: {
					type: Boolean,
					description:
						'Skip creating a backup before applying fixes (alias: --no-backup)',
				},
			},
		})

		const restore = command({
			name: 'restore',
			alias: 'r',
			help: {
				description: 'Restore files from a previous backup snapshot',
			},
			flags: {
				config: {
					type: String,
					alias: 'c',
					description:
						'Path to kuizer config file (defaults to kuizer.config.json) (alias: --config)',
				},
				tsconfig: {
					type: String,
					description: 'Path to tsconfig.json (defaults to auto-detection)',
				},
				snapshot: {
					type: String,
					description: 'Path to snapshot manifest (JSON file) or snapshot ID',
				},
			},
		})

		const init = command({
			name: 'init',
			help: {
				description: 'Interactive configuration setup',
			},
			flags: {
				global: {
					type: Boolean,
					description: 'Create global config (~/.kuizerrc)',
				},
				project: {
					type: Boolean,
					description: 'Create project config (./kuizer.config.json)',
				},
			},
		})

		const revert = command({
			name: 'revert',
			alias: 'rv',
			help: {
				description: 'Revert changes from a snapshot',
			},
			flags: {
				snapshot: {
					type: String,
					description: 'Snapshot path or ID to revert from',
				},
			},
		})

		const parsed = cli(
			{
				name: 'kuizer',
				version,
				help: false,
				commands: [detect, fix, restore, init, revert],
			},
			undefined,
			normalizedArgv
		)

		if ('command' in parsed) {
			const command = parsed.command as string
			const errors = collectFlagValidationErrors(command, parsed.flags as FlagRecord)
			if (errors.length > 0) {
				console.error('kuizer: Error:')
				errors.forEach((error) => console.error(`  ${error}`))
				console.error('')
				console.error('For help, use: kuizer --help')
				return 1
			}
		}

		if (!('command' in parsed)) {
			printMainHelp(version)
			return 1
		}

		if (parsed.command === 'detect') {
			return runDetect(parsed.flags)
		}

		if (parsed.command === 'fix') {
			return runFix(parsed.flags)
		}

		if (parsed.command === 'restore') {
			return runRestore(parsed.flags)
		}

		if (parsed.command === 'init') {
			return runInit(parsed.flags)
		}

		if (parsed.command === 'revert') {
			return runRevert(parsed.flags)
		}

		printMainHelp(version)
		return 1
	} catch (error) {
		if (error instanceof CliError) {
			console.error(`kuizer: ${error.message}`)
			return error.exitCode
		}

		throw error
	}
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
	try {
		process.exitCode = await main()
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error)
		console.error(`kuizer: ${message}`)
		process.exitCode = 2
	}
}
