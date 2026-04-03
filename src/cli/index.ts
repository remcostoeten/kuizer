#!/usr/bin/env node

import { cli, command } from 'cleye'
import { fileURLToPath } from 'node:url'
import readline from 'node:readline'
import { dim, write, bold, green, cyan, yellow, red } from '../output/index.js'
import { restoreSnapshot } from '../backup/restore.js'
import { loadConfig } from '../config/index.js'
import { registry as detectorRegistry } from '../detectors/registry.js'
import { applyFixPlans } from '../fixers/apply.js'
import { bootstrap } from '../engine/index.js'
import { planFixes } from '../fixers/plan.js'
import { reportAppliedFixes, reportFixPlan } from '../fixers/report.js'
import { getVersion } from '../meta.js'
import { createProgress } from '../output/progress.js'
import { registry as reporterRegistry } from '../reporters/registry.js'
import type { Config, Finding } from '../types.js'

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
	snapshot: string | undefined
}

type DetectorSelectionFlags = Pick<DetectFlags, 'only' | 'skip'>

function parseCsv(input?: string): string[] {
	if (!input) return []

	return input
		.split(',')
		.map((part) => part.trim())
		.filter(Boolean)
}

function resolveReporter(flags: DetectFlags): ReporterName {
	if (flags.json) return 'json'

	const reporter = flags.reporter ?? 'terminal'
	if (reporter in reporterRegistry) {
		return reporter as ReporterName
	}

	throw new Error(`Unknown reporter '${reporter}'`)
}

function resolveDetectorIds(config: Config, flags: DetectorSelectionFlags): DetectorId[] {
	const available = Object.keys(detectorRegistry) as DetectorId[]
	const only = parseCsv(flags.only)
	const skip = new Set(parseCsv(flags.skip))
	const enabled = available.filter((id) => config.detectors[id]?.enabled !== false)

	for (const detectorId of [...only, ...skip]) {
		if (!available.includes(detectorId as DetectorId)) {
			throw new Error(`Unknown detector '${detectorId}'`)
		}
	}

	return (only.length > 0 ? only : enabled).filter((id): id is DetectorId => !skip.has(id))
}

function exitCode(findings: readonly Finding[]): number {
	return findings.some((finding) => finding.severity === 'error') ? 1 : 0
}

function groupByFileInteractive(findings: readonly Finding[]): Map<string, Finding[]> {
	return findings.reduce((acc, f) => {
		const group = acc.get(f.file) ?? []
		group.push(f)
		return acc.set(f.file, group)
	}, new Map<string, Finding[]>())
}

async function runDetect(flags: DetectFlags): Promise<number> {
	const startedAt = Date.now()
	const config = {
		...(await loadConfig(process.cwd(), flags.config)),
		...(flags.tsconfig ? { tsconfig: flags.tsconfig } : {}),
	} satisfies Config

	const reporterName = resolveReporter(flags)
	const interactive = flags.interactive === true
	const showProgress = reporterName === 'terminal' && !interactive
	const progress = showProgress ? createProgress({ cwd: process.cwd() }) : undefined

	progress?.update('Loading TypeScript project')
	const ctx = await bootstrap(config, {
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
					progress.update('Resolving entrypoints', `${event.totalFiles ?? 0} files`)
					break
				case 'analyze-file':
					progress.update(
						`Analyzing import graph ${event.completed}/${event.totalFiles}`,
						event.file
					)
					break
				case 'complete':
					progress.update('Analysis ready', `${event.totalFiles ?? 0} files`)
					break
			}
		},
	})
	const detectors = resolveDetectorIds(config, flags)
	let findings: Finding[] = []

	for (const [index, id] of detectors.entries()) {
		progress?.update(`Running detector ${index + 1}/${detectors.length}`, id)
		findings.push(...detectorRegistry[id](ctx, config))
	}

	if (flags.filter) {
		const kindMap: Record<string, string> = {
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
		const filterKinds = parseCsv(flags.filter).flatMap((k) => {
			const normalized = k.toLowerCase()
			return kindMap[normalized] ? [kindMap[normalized]] : [normalized]
		})
		if (!filterKinds.includes('all')) {
			findings = findings.filter((f) => {
				if (f.detectorId !== 'unusedExports') return true
				return f.kind && filterKinds.includes(f.kind)
			})
		}
	}

	if (flags.excludePattern) {
		try {
			const regex = new RegExp(flags.excludePattern)
			findings = findings.filter((f) => !regex.test(f.symbol || ''))
		} catch (e) {
			throw new Error(`Invalid regex pattern: ${flags.excludePattern}`)
		}
	}

	if (flags.interactive) {
		type ViewMode = 'summary' | 'results' | 'help'

		const W = 56

		const applyFilter = (selectedKinds: string[], allFindings: Finding[]) => {
			if (selectedKinds.includes('all') || selectedKinds.length === 0) {
				return allFindings
			}
			return allFindings.filter((f) => {
				if (f.detectorId !== 'unusedExports') return true
				return f.kind && selectedKinds.includes(f.kind)
			})
		}

		const pad = (s: string, len: number) => s + ' '.repeat(Math.max(0, len - s.length))
		const padStart = (s: string, len: number) => ' '.repeat(Math.max(0, len - s.length)) + s

		const renderDashboard = (
			currentKinds: string[],
			filteredFindings: Finding[],
			view: ViewMode,
			page: number
		) => {
			const pageSize = 12
			const grouped = groupByFileInteractive(filteredFindings)
			const fileCount = grouped.size
			const totalPages = Math.max(1, Math.ceil(fileCount / pageSize))
			const clampedPage = Math.max(1, Math.min(page, totalPages))

			const errors = filteredFindings.filter((f) => f.severity === 'error').length
			const warnings = filteredFindings.filter((f) => f.severity === 'warn').length
			const funcs = filteredFindings.filter((f) => f.kind === 'function').length
			const types = filteredFindings.filter(
				(f) => f.kind === 'type' || f.kind === 'interface'
			).length
			const consts = filteredFindings.filter((f) => f.kind === 'const').length

			process.stdout.write('\x1b[2J\x1b[H')

			write(bold(pad(` kuizer ${version}`, W - 1) + '│'))
			write(bold(pad('│', W) + '│'))
			write(bold(pad('│ Detect unused exports', W - 1) + '│'))
			write(bold(pad('│', W) + '│'))
			write(bold(pad(`│ ${filteredFindings.length} findings`, W - 1) + '│'))
			write(bold(pad(`│   ${errors} errors  ${warnings} warnings`, W - 1) + '│'))
			write(bold(pad('│', W) + '│'))
			write(bold(pad('│ Filters:', W - 1) + '│'))
			write(bold(pad(`│   [${currentKinds.includes('function') ? 'x' : ' '}] fn  [${currentKinds.includes('const') ? 'x' : ' '}] const  [${currentKinds.includes('type') ? 'x' : ' '}] type`, W - 1) + '│'))
			write(bold(pad(`│   [${currentKinds.includes('interface') ? 'x' : ' '}] interface`, W - 1) + '│'))
			write(bold(pad('│', W) + '│'))
			write(bold(pad('│ View:', W - 1) + '│'))
			write(bold(pad(`│   [${view === 'summary' ? 'x' : ' '}] summary  [${view === 'results' ? 'x' : ' '}] results  [${view === 'help' ? 'x' : ' '}] help`, W - 1) + '│'))
			write(bold(pad('', W) + '│'))
			write(bold('└' + '─'.repeat(W - 1) + '┘'))

			if (view === 'help') {
				write(bold('\n┌' + '─'.repeat(W - 2) + '┐'))
				write(bold(pad('│ Keyboard shortcuts', W - 1) + '│'))
				write(bold('├' + '─'.repeat(W - 2) + '┤'))
				write(pad('│ f   toggle functions', W - 1) + '│')
				write(pad('│ c   toggle const', W - 1) + '│')
				write(pad('│ t   toggle types', W - 1) + '│')
				write(pad('│ i   toggle interfaces', W - 1) + '│')
				write(pad('│ a   show all kinds', W - 1) + '│')
				write(pad('│ r   reset filters', W - 1) + '│')
				write(bold('├' + '─'.repeat(W - 2) + '┤'))
				write(pad('│ v   switch view', W - 1) + '│')
				write(pad('│ ←→  navigate pages', W - 1) + '│')
				write(bold('├' + '─'.repeat(W - 2) + '┤'))
				write(pad('│ Enter apply selected', W - 1) + '│')
				write(pad('│ Space select file', W - 1) + '│')
				write(bold('├' + '─'.repeat(W - 2) + '┤'))
				write(pad('│ q   quit', W - 1) + '│')
				write(bold('└' + '─'.repeat(W - 2) + '┘'))
			} else if (view === 'summary') {
				write(bold('\n┌' + '─'.repeat(W - 2) + '┐'))
				write(bold(pad(`│ Files with unused exports (${fileCount})`, W - 1) + '│'))
				write(bold('├' + '─'.repeat(W - 2) + '┤'))
				write(pad('│ ' + padStart(`${funcs}`, 3) + ' functions', W - 1) + '│')
				write(pad('│ ' + padStart(`${consts}`, 3) + ' const', W - 1) + '│')
				write(pad('│ ' + padStart(`${types}`, 3) + ' types', W - 1) + '│')
				write(bold('├' + '─'.repeat(W - 2) + '┤'))
				write(pad('│ ' + padStart(`${clampedPage}`, 3) + ' / ' + `${totalPages}` + ' pages', W - 1) + '│')
				write(bold('└' + '─'.repeat(W - 2) + '┘'))

				write(bold('\n┌' + '─'.repeat(W - 2) + '┐'))
				write(bold(pad('│ Files', W - 1) + '│'))
				write(bold('├' + '─'.repeat(W - 2) + '┤'))

				const allFiles = Array.from(grouped.entries())
				const startIdx = (clampedPage - 1) * pageSize
				const pageFiles = allFiles.slice(startIdx, startIdx + pageSize)

				for (const [file, group] of pageFiles) {
					const shortFile = file.length > 40 ? '...' + file.slice(-37) : file
					write(pad(`│ ${padStart(`${group.length}`, 2)} │ ${shortFile}`, W - 1) + '│')
				}

				if (pageFiles.length === 0) {
					write(pad('│ (no files)', W - 1) + '│')
				}

				write(bold('└' + '─'.repeat(W - 2) + '┘'))
			} else {
				const allFiles = Array.from(grouped.entries())
				const startIdx = (clampedPage - 1) * pageSize
				const pageFiles = allFiles.slice(startIdx, startIdx + pageSize)

				write(bold('\n┌' + '─'.repeat(W - 2) + '┐'))
				write(bold(pad(`│ Results (page ${clampedPage}/${totalPages})`, W - 1) + '│'))
				write(bold('└' + '─'.repeat(W - 2) + '┘'))

				for (const [file, group] of pageFiles) {
					const shortFile = file.length > 40 ? '...' + file.slice(-37) : file
					write(bold('\n┌' + '─'.repeat(W - 2) + '┐'))
					write(bold(pad(`│ ${shortFile}`, W - 1) + '│'))
					write(bold('├' + '─'.repeat(W - 2) + '┤'))

					for (const f of group.slice(0, 5)) {
						const kindLabel = f.kind === 'function'
							? 'fn'
							: f.kind === 'type'
								? 'tp'
								: f.kind === 'interface'
									? 'if'
									: f.kind === 'const'
										? 'cn'
										: '--'
						const sym = f.symbol?.slice(0, 24) || '?'
						const lineInfo = `L${f.line}`
						write(pad(`│ ${padStart(lineInfo, 4)} [${kindLabel}] ${sym}`, W - 1) + '│')
					}
					if (group.length > 5) {
						write(dim(pad(`│   + ${group.length - 5} more...`, W - 1) + '│'))
					}
				}

				write(bold('\n└' + '─'.repeat(W - 2) + '┘'))
			}

			write(bold('\n› '))
		}

		function groupedCount(findings: Finding[]): number {
			const grouped = groupByFileInteractive(findings)
			return grouped.size
		}

		readline.emitKeypressEvents(process.stdin)
		process.stdin.setRawMode(true)

		let running = true
		let activeKinds: string[] = []
		let view: ViewMode = 'summary'
		let page = 1

		while (running) {
			const filtered = applyFilter(activeKinds, findings)
			renderDashboard(activeKinds, filtered, view, page)

			const key = await new Promise<string>((resolve) => {
				const handler = (str: string, key: { name: string; meta: boolean }) => {
					process.stdin.off('keypress', handler)
					resolve(key.name || str)
				}
				process.stdin.on('keypress', handler)
			})

			switch (key) {
				case 'f':
					activeKinds = activeKinds.includes('function')
						? activeKinds.filter((k) => k !== 'function')
						: [...activeKinds, 'function']
					page = 1
					break
				case 'c':
					activeKinds = activeKinds.includes('const')
						? activeKinds.filter((k) => k !== 'const')
						: [...activeKinds, 'const']
					page = 1
					break
				case 't':
					activeKinds = activeKinds.includes('type')
						? activeKinds.filter((k) => k !== 'type')
						: [...activeKinds, 'type']
					page = 1
					break
				case 'i':
					activeKinds = activeKinds.includes('interface')
						? activeKinds.filter((k) => k !== 'interface')
						: [...activeKinds, 'interface']
					page = 1
					break
				case 'a':
					activeKinds = activeKinds.includes('all') ? [] : ['all']
					page = 1
					break
				case 'r':
					activeKinds = []
					page = 1
					break
				case 'v':
					view = view === 'summary' ? 'results' : view === 'results' ? 'help' : 'summary'
					page = 1
					break
				case '?':
					view = view === 'help' ? 'summary' : 'help'
					page = 1
					break
				case 'right':
				case 'arrowright':
					page++
					break
				case 'left':
				case 'arrowleft':
					page = Math.max(1, page - 1)
					break
				case 'q':
				case 'escape':
					running = false
					break
			}
		}

		process.stdin.setRawMode(false)
		write(dim('\nGoodbye!'))
		return 0
	}

	progress?.stop()

	const reporter = reporterRegistry[reporterName]

	reporter({
		findings,
		durationMs: Date.now() - startedAt,
		analysisBoundaries: ctx.analysisBoundaries,
	})
	return exitCode(findings)
}

async function runFix(flags: FixFlags): Promise<number> {
	const config = {
		...(await loadConfig(process.cwd(), flags.config)),
		...(flags.tsconfig ? { tsconfig: flags.tsconfig } : {}),
	} satisfies Config

	const ctx = await bootstrap(config)
	const detectors = resolveDetectorIds(config, flags)
	const findings = detectors.flatMap((id) => detectorRegistry[id](ctx, config))
	const plans = planFixes(findings, ctx, config)

	const shouldWrite = flags.write === true
	if (!shouldWrite || flags.dryRun === true) {
		reportFixPlan(plans, { dryRun: true })
		return 0
	}

	const result = applyFixPlans(plans.supported, plans.unsupported, config, {
		backup: flags.noBackup !== true,
	})
	reportAppliedFixes(result)
	return 0
}

async function runRestore(flags: RestoreFlags): Promise<number> {
	if (!flags.snapshot) {
		throw new Error('Missing required --snapshot path')
	}

	const config = await loadConfig(process.cwd(), flags.config)
	const result = restoreSnapshot(flags.snapshot, {
		cwd: process.cwd(),
		outputDir: config.outputDir,
	})
	write(`Restored ${result.restored.length} file${result.restored.length === 1 ? '' : 's'}`)
	write(dim(`Snapshot: ${result.manifestPath}`))
	return 0
}

export async function main(argv = process.argv.slice(2)): Promise<number> {
	const detect = command({
		name: 'detect',
		help: {
			description: 'Analyze your codebase to find unused exports and dead code',
		},
		flags: {
			config: {
				type: String,
				description: 'Path to kuizer config file (defaults to kuizer.config.json)',
			},
			tsconfig: {
				type: String,
				description: 'Path to tsconfig.json (auto-detected if not provided)',
			},
			reporter: {
				type: String,
				description: 'Output format: terminal (colored) or json',
			},
			only: {
				type: String,
				description: 'Run only specific detectors (e.g., unusedExports, unusedFiles)',
			},
			skip: {
				type: String,
				description: 'Skip specific detectors',
			},
			json: {
				type: Boolean,
				description: 'Output results as JSON (shorthand for --reporter json)',
			},
			filter: {
				type: String,
				description:
					'Filter results by export type: functions, const, types, interfaces, classes, all',
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
		help: {
			description: 'Automatically remove unused exports and dead code from your project',
		},
		flags: {
			config: {
				type: String,
				description: 'Path to kuizer config file (defaults to kuizer.config.json)',
			},
			tsconfig: {
				type: String,
				description: 'Path to tsconfig.json (auto-detected if not provided)',
			},
			only: {
				type: String,
				description: 'Plan fixes for specific detectors only',
			},
			skip: {
				type: String,
				description: 'Skip specific detectors when planning fixes',
			},
			dryRun: {
				type: Boolean,
				description: 'Show what would be fixed without writing any changes',
			},
			write: {
				type: Boolean,
				description: 'Apply the fixes to your files (creates a backup by default)',
			},
			noBackup: {
				type: Boolean,
				description: 'Skip creating a backup before applying fixes',
			},
		},
	})

	const restore = command({
		name: 'restore',
		help: {
			description: 'Restore files from a previous backup snapshot',
		},
		flags: {
			config: {
				type: String,
				description: 'Path to kuizer config file (defaults to kuizer.config.json)',
			},
			snapshot: {
				type: String,
				description: 'Path to snapshot manifest (JSON file) or snapshot ID',
			},
		},
	})

	const parsed = cli(
		{
			name: 'kuizer',
			version,
			help: {
				description: 'TypeScript dead code intelligence for large codebases.',
			},
			commands: [detect, fix, restore],
		},
		undefined,
		argv
	)

	if (!('command' in parsed)) {
		;(parsed as { showHelp(): void }).showHelp()
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

	;(parsed as { showHelp(): void }).showHelp()
	return 1
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
