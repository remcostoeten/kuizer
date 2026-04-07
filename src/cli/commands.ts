import { existsSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { resolve } from 'node:path'
import pc from 'picocolors'
import { preset } from '../config/presets.js'
import { applyFixPlans } from '../fixers/apply.js'
import { planFixes } from '../fixers/plan.js'
import { reportAppliedFixes, reportFixPlan } from '../fixers/report.js'
import type { Ctx, Finding, Framework } from '../types.js'
import { bootstrap } from '../engine/index.js'
import { createProgress } from '../output/progress.js'
import { registry as detectorRegistry } from '../detectors/registry.js'
import { registry as reporterRegistry } from '../reporters/registry.js'
import { restoreSnapshot } from '../backup/restore.js'
import { loadConfig } from '../config/index.js'
import { dim, write } from '../output/index.js'
import type {
	DetectFlags,
	FixFlags,
	RestoreFlags,
	InitFlags,
	RevertFlags,
} from './parser.js'
import {
	resolveReporter,
	resolveDetectorIds,
	resolveFilterSelection,
	applyFindingFilters,
	compileExcludePattern,
	applyExcludePattern,
} from './parser.js'
import { runInteractiveSession } from './interactive.js'

function exitCode(findings: readonly Finding[]): number {
	return findings.some((finding) => finding.severity === 'error') ? 1 : 0
}

export async function runDetect(flags: DetectFlags, version: string): Promise<number> {
	const startedAt = Date.now()
	const config = await loadConfig(process.cwd(), flags.config, flags.tsconfig)

	const reporterName = resolveReporter(flags)
	const interactive = flags.interactive === true
	const showProgress = reporterName === 'terminal' && !interactive
	const progress = showProgress ? createProgress({ cwd: process.cwd() }) : undefined
	const detectors = resolveDetectorIds(config, flags.only ? { only: flags.only, skip: flags.skip ?? '' } : { only: '', skip: flags.skip ?? '' })
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

		const findings: Finding[] = []
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
			throw error
		}

		throw error
	} finally {
		progress?.stop()
	}
}

export async function runFix(flags: FixFlags): Promise<number> {
	const config = await loadConfig(process.cwd(), flags.config, flags.tsconfig)

	try {
		const ctx = await bootstrap(config)
	const detectors = resolveDetectorIds(config, flags.only ? { only: flags.only, skip: flags.skip ?? '' } : { only: '', skip: flags.skip ?? '' })
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
			throw error
		}

		throw error
	}
}

export async function runRestore(flags: RestoreFlags): Promise<number> {
	if (!flags.snapshot) {
		throw new Error('Missing required --snapshot path')
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

export async function runInit(flags: InitFlags): Promise<number> {
	let isGlobal: boolean
	if (flags.global && flags.project) {
		console.error(pc.red('Error: Cannot specify both --global and --project'))
		return 1
	} else if (flags.global) {
		isGlobal = true
	} else if (flags.project) {
		isGlobal = false
	} else {
		console.log(pc.yellow('🔧 Kuizer Configuration Setup'))
		console.log('')
		console.log('Where do you want to create the configuration?')
		console.log('1) Global (~/.kuizerrc) - applies to all projects')
		console.log('2) Project (./kuizer.config.json) - applies to current project only')
		console.log('')

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

	if (existsSync(configPath)) {
		console.log(pc.red(`Configuration already exists at: ${configPath}`))
		console.log(pc.dim('Remove it first or edit it directly.'))
		return 1
	}

	console.log('')
	console.log(pc.yellow('Configure your preferences:'))
	console.log('')

	process.stdout.write(pc.cyan('Default to dry-run mode? (recommended) [Y/n]: '))
	const dryRunChoice = await new Promise<string>((resolve) => {
		process.stdin.once('data', (data) => {
			resolve(data.toString().trim().toLowerCase())
		})
	})
	const dryRunByDefault = dryRunChoice !== 'n' && dryRunChoice !== 'no'

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

	const config = {
		dryRunByDefault,
		framework,
		...(framework !== 'none' && preset(framework)),
	}

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

export async function runRevert(flags: RevertFlags): Promise<number> {
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
