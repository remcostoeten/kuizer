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
import { restoreSnapshot } from '../backup/restore.js'
import { loadConfig } from '../config/index.js'
import { dim, write } from '../output/index.js'
import type {
	DetectFlags,
	FixFlags,
	RestoreFlags,
	InitFlags,
	CompletionFlags,
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

const PRESET_UI_IGNORE_PATHS = new Set(['**/ui/**', '**/components/ui/**'])

function applyCliOverrides<T extends { includeUi?: boolean | undefined }>(
	config: Config,
	flags: T
): Config {
	if (flags.includeUi !== true) {
		return config
	}

	return {
		...config,
		ignore: {
			...config.ignore,
			paths: config.ignore.paths.filter((path) => !PRESET_UI_IGNORE_PATHS.has(path)),
		},
	}
}

function exitCode(findings: readonly Finding[]): number {
	return findings.some((finding) => finding.severity === 'error') ? 1 : 0
}

export async function runDetect(flags: DetectFlags, version: string): Promise<number> {
	const startedAt = Date.now()
	const config = applyCliOverrides(
		await loadConfig(process.cwd(), flags.config, flags.tsconfig),
		flags
	)

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
	const config = applyCliOverrides(
		await loadConfig(process.cwd(), flags.config, flags.tsconfig),
		flags
	)

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

type CompletionShell = 'bash' | 'zsh' | 'fish'

const COMPLETION_SHELLS = ['bash', 'zsh', 'fish'] as const satisfies readonly CompletionShell[]

function isCompletionShell(value: string): value is CompletionShell {
	return COMPLETION_SHELLS.includes(value as CompletionShell)
}

function shellCommands(): readonly string[] {
	return ['detect', 'fix', 'restore', 'init', 'completion', 'revert', 'info', 'help', 'version']
}

function shellFlags(): readonly string[] {
	return [
		'--help',
		'-h',
		'--version',
		'-v',
		'--config',
		'-c',
		'--tsconfig',
		'--only',
		'-o',
		'--skip',
		'-s',
		'--reporter',
		'-r',
		'--report',
		'--json',
		'--filter',
		'-f',
		'--exclude-pattern',
		'--interactive',
		'--include-ui',
		'--dry',
		'--dr',
		'--dry-run',
		'--write',
		'-w',
		'--no-backup',
		'--snapshot',
		'--global',
		'--project',
		'--shell',
	]
}

function generateBashCompletion(): string {
	const commands = shellCommands().join(' ')
	const flags = shellFlags().join(' ')

	return [
		'# bash completion for kuizer',
		'_kuizer_completions() {',
		'\tlocal cur prev words cword',
		'\t_init_completion || return',
		'',
		`\tlocal commands="${commands}"`,
		`\tlocal global_flags="${flags}"`,
		'',
		'\tif [[ "$prev" == "--shell" ]]; then',
		'\t\tCOMPREPLY=( $(compgen -W "bash zsh fish" -- "$cur") )',
		'\t\treturn',
		'\tfi',
		'',
		'\tcase "${words[1]}" in',
		'\t\tdetect)',
		'\t\t\tCOMPREPLY=( $(compgen -W "$global_flags" -- "$cur") )',
		'\t\t\treturn',
		'\t\t\t;;',
		'\t\tfix)',
		'\t\t\tCOMPREPLY=( $(compgen -W "$global_flags" -- "$cur") )',
		'\t\t\treturn',
		'\t\t\t;;',
		'\t\trestore|revert)',
		'\t\t\tCOMPREPLY=( $(compgen -W "--snapshot --config --tsconfig" -- "$cur") )',
		'\t\t\treturn',
		'\t\t\t;;',
		'\t\tinit)',
		'\t\t\tCOMPREPLY=( $(compgen -W "--global --project" -- "$cur") )',
		'\t\t\treturn',
		'\t\t\t;;',
		'\t\tcompletion)',
		'\t\t\tCOMPREPLY=( $(compgen -W "--shell -s" -- "$cur") )',
		'\t\t\treturn',
		'\t\t\t;;',
		'\t\tinfo|help|version)',
		'\t\t\treturn',
		'\t\t\t;;',
		'\tesac',
		'',
		'\tif [[ $cword -eq 1 ]]; then',
		'\t\tCOMPREPLY=( $(compgen -W "$commands $global_flags" -- "$cur") )',
		'\t\treturn',
		'\tfi',
		'',
		'\tCOMPREPLY=( $(compgen -W "$global_flags" -- "$cur") )',
		'}',
		'',
		'complete -F _kuizer_completions kuizer',
		'',
	].join('\n')
}

function generateZshCompletion(): string {
	const commands = shellCommands()

	return `#compdef kuizer

_kuizer() {
	local -a commands
	commands=(
${commands.map((command) => `\t\t'${command}:${command} command'`).join('\n')}
	)

	_arguments -C \
		'1:command:->command' \
		'*::arg:->args'

	case $state in
		command)
			_describe 'command' commands
			;;
		args)
			case $words[2] in
				completion)
					_arguments '--shell[Shell to generate completions for]:shell:(bash zsh fish)' '-s[Shell to generate completions for]:shell:(bash zsh fish)'
					;;
				init)
					_arguments '--global[Create global config]' '--project[Create project config]'
					;;
				restore|revert)
					_arguments '--snapshot[Snapshot path or ID]' '--config[Config path]' '--tsconfig[tsconfig path]'
					;;
				detect|fix)
					_arguments \
						'--config[Config path]' '-c[Config path]' \
						'--tsconfig[tsconfig path]' \
						'--only[Run only specific detectors]' '-o[Run only specific detectors]' \
						'--skip[Skip specific detectors]' '-s[Skip specific detectors]' \
						'--include-ui[Include preset ui directories in analysis]' \
						'--dry-run[Preview fixes without writing]' '--dry[Preview fixes without writing]' '--dr[Preview fixes without writing]' \
						'--write[Apply fixes to files]' '-w[Apply fixes to files]' \
						'--no-backup[Skip backup before applying fixes]' \
						'--reporter[Output format]:format:(terminal json)' '-r[Output format]:format:(terminal json)' \
						'--report[Output format]:format:(terminal json)' \
						'--json[Output results as JSON]' \
						'--filter[Filter by kind or detector]' '-f[Filter by kind or detector]' \
						'--exclude-pattern[Exclude symbols matching regex]' \
						'--interactive[Launch interactive TUI]'
					;;
			esac
			;;
	esac
}

_kuizer "$@"
`
}

function generateFishCompletion(): string {
	return `complete -c kuizer -f
complete -c kuizer -n "__fish_use_subcommand" -a "detect fix restore init completion revert info help version"
complete -c kuizer -n "__fish_seen_subcommand_from completion" -l shell -s s -a "bash zsh fish"
complete -c kuizer -n "__fish_seen_subcommand_from init" -l global
complete -c kuizer -n "__fish_seen_subcommand_from init" -l project
complete -c kuizer -n "__fish_seen_subcommand_from restore revert" -l snapshot
complete -c kuizer -n "__fish_seen_subcommand_from detect fix restore" -l config -s c
complete -c kuizer -n "__fish_seen_subcommand_from detect fix restore" -l tsconfig
complete -c kuizer -n "__fish_seen_subcommand_from detect fix" -l only -s o
complete -c kuizer -n "__fish_seen_subcommand_from detect fix" -l skip -s s
complete -c kuizer -n "__fish_seen_subcommand_from detect" -l reporter -s r -a "terminal json"
complete -c kuizer -n "__fish_seen_subcommand_from detect" -l report -a "terminal json"
complete -c kuizer -n "__fish_seen_subcommand_from detect" -l json
complete -c kuizer -n "__fish_seen_subcommand_from detect" -l filter -s f
complete -c kuizer -n "__fish_seen_subcommand_from detect" -l exclude-pattern
complete -c kuizer -n "__fish_seen_subcommand_from detect" -l interactive
complete -c kuizer -n "__fish_seen_subcommand_from detect fix" -l include-ui
complete -c kuizer -n "__fish_seen_subcommand_from fix" -l dry-run
complete -c kuizer -n "__fish_seen_subcommand_from fix" -l dry
complete -c kuizer -n "__fish_seen_subcommand_from fix" -l dr
complete -c kuizer -n "__fish_seen_subcommand_from fix" -l write -s w
complete -c kuizer -n "__fish_seen_subcommand_from fix" -l no-backup
`
}

function completionScript(shell: CompletionShell): string {
	switch (shell) {
		case 'bash':
			return generateBashCompletion()
		case 'zsh':
			return generateZshCompletion()
		case 'fish':
			return generateFishCompletion()
	}
}

function printCompletionInstructions(): void {
	write('Shell completions')
	write(dim('Generate a completion script with:'))
	write('  kuizer completion --shell bash')
	write('  kuizer completion --shell zsh')
	write('  kuizer completion --shell fish')
	write('')
	write(dim('Install examples:'))
	write('  Bash: kuizer completion --shell bash > ~/.local/share/bash-completion/completions/kuizer')
	write('  Zsh:  kuizer completion --shell zsh > ~/.zfunc/_kuizer')
	write('        Then add `fpath=(~/.zfunc $fpath)` to your ~/.zshrc and run `autoload -Uz compinit && compinit`')
	write('  Fish: kuizer completion --shell fish > ~/.config/fish/completions/kuizer.fish')
}

export async function runCompletion(flags: CompletionFlags): Promise<number> {
	if (!flags.shell) {
		printCompletionInstructions()
		return 0
	}

	const shell = flags.shell.toLowerCase()
	if (!isCompletionShell(shell)) {
		console.error(pc.red(`Error: Unsupported shell '${flags.shell}'`))
		console.error(pc.dim('Use one of: bash, zsh, fish'))
		return 1
	}

	process.stdout.write(completionScript(shell))
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

export async function runInfo(): Promise<number> {
	const config = await loadConfig(process.cwd())

	console.log('')
	console.log(pc.bold('Kuizer Configuration'))
	console.log(pc.dim('─'.repeat(40)))
	console.log('')
	console.log(pc.cyan('Framework:'), config.framework)
	console.log('')
	console.log(pc.cyan('Entrypoints:'))
	if (config.entrypoints.length === 0) {
		console.log(pc.dim('  (inferred)'))
	} else {
		for (const entry of config.entrypoints) {
			console.log(`  ${entry}`)
		}
	}
	console.log('')
	console.log(pc.cyan('Allowed Exports:'))
	if (config.allowedExports.length === 0) {
		console.log(pc.dim('  (none)'))
	} else {
		for (const exp of config.allowedExports) {
			console.log(`  ${exp}`)
		}
	}
	console.log('')
	console.log(pc.cyan('Ignored Paths:'))
	if (config.ignore.paths.length === 0) {
		console.log(pc.dim('  (none)'))
	} else {
		for (const path of config.ignore.paths) {
			console.log(`  ${path}`)
		}
	}
	console.log('')
	console.log(pc.cyan('Ignored Files:'))
	if (config.ignore.files.length === 0) {
		console.log(pc.dim('  (none)'))
	} else {
		for (const file of config.ignore.files) {
			console.log(`  ${file}`)
		}
	}
	console.log('')
	console.log(pc.cyan('Ignored Symbols:'))
	if (config.ignore.symbols.length === 0) {
		console.log(pc.dim('  (none)'))
	} else {
		for (const sym of config.ignore.symbols) {
			console.log(`  ${sym}`)
		}
	}
	console.log('')
	console.log(pc.cyan('Other Options:'))
	console.log(`  countTestsAsConsumers: ${config.countTestsAsConsumers}`)
	console.log(`  dryRunByDefault: ${config.dryRunByDefault}`)
	console.log('')

	return 0
}
