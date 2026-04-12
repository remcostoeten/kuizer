import readline from 'node:readline'
import pc from 'picocolors'
import { loadConfig } from '../config/index.js'
import type { Config } from '../types.js'
import { runDetect, runFix, runInfo, runRestore } from './commands.js'
import type { DetectFlags, FixFlags, RestoreFlags } from './parser.js'

type MenuName = 'main' | 'detect' | 'fix' | 'restore'
type MainAction = 'detect' | 'fix' | 'restore' | 'info' | 'help' | 'quit'
type ItemKind = 'input' | 'toggle' | 'choice' | 'action'

type KeyInput = Readonly<{
	str: string
	name?: string
	ctrl?: boolean
}>

type MenuItem = Readonly<{
	label: string
	description: string
	value?: string
	kind: ItemKind
	field?: string
	choices?: readonly string[]
	action?: MainAction | 'run' | 'back'
}>

type MainMenuOptions = Readonly<{
	input?: NodeJS.ReadStream
	output?: NodeJS.WriteStream
	config?: Config
	version?: string
}>

function emptyDetectFlags(): DetectFlags {
	return {
		config: undefined,
		tsconfig: undefined,
		only: undefined,
		skip: undefined,
		reporter: undefined,
		json: undefined,
		filter: undefined,
		excludePattern: undefined,
		interactive: undefined,
		includeUi: undefined,
	}
}

function emptyFixFlags(): FixFlags {
	return {
		config: undefined,
		tsconfig: undefined,
		only: undefined,
		skip: undefined,
		dryRun: true,
		write: false,
		noBackup: undefined,
		includeUi: undefined,
	}
}

function emptyRestoreFlags(): RestoreFlags {
	return {
		config: undefined,
		tsconfig: undefined,
		snapshot: undefined,
	}
}

function boolLabel(value: boolean | undefined): string {
	return value === true ? 'yes' : 'no'
}

function valueLabel(value: string | undefined, fallback = '(auto)'): string {
	return value && value.length > 0 ? value : fallback
}

function detectCommandPreview(flags: DetectFlags): string {
	const parts = ['kuizer', 'detect']

	if (flags.config) parts.push('--config', flags.config)
	if (flags.tsconfig) parts.push('--tsconfig', flags.tsconfig)
	if (flags.only) parts.push('--only', flags.only)
	if (flags.skip) parts.push('--skip', flags.skip)
	if (flags.filter) parts.push('--filter', flags.filter)
	if (flags.excludePattern) parts.push('--exclude-pattern', flags.excludePattern)
	if (flags.json) {
		parts.push('--json')
	} else if (flags.reporter) {
		parts.push('--reporter', flags.reporter)
	}
	if (flags.interactive) parts.push('--interactive')
	if (flags.includeUi) parts.push('--include-ui')

	return parts.join(' ')
}

function fixCommandPreview(flags: FixFlags): string {
	const parts = ['kuizer', 'fix']

	if (flags.config) parts.push('--config', flags.config)
	if (flags.tsconfig) parts.push('--tsconfig', flags.tsconfig)
	if (flags.only) parts.push('--only', flags.only)
	if (flags.skip) parts.push('--skip', flags.skip)
	if (flags.write) {
		parts.push('--write')
		if (flags.noBackup) {
			parts.push('--no-backup')
		}
	} else {
		parts.push('--dry-run')
	}
	if (flags.includeUi) parts.push('--include-ui')

	return parts.join(' ')
}

function restoreCommandPreview(flags: RestoreFlags): string {
	const parts = ['kuizer', 'restore']

	if (flags.config) parts.push('--config', flags.config)
	if (flags.tsconfig) parts.push('--tsconfig', flags.tsconfig)
	if (flags.snapshot) parts.push('--snapshot', flags.snapshot)

	return parts.join(' ')
}

function detectMenuItems(flags: DetectFlags): MenuItem[] {
	return [
		{
			label: 'Config Path',
			description: 'Override config discovery',
			value: valueLabel(flags.config),
			kind: 'input',
			field: 'config',
		},
		{
			label: 'Tsconfig Path',
			description: 'Override tsconfig detection',
			value: valueLabel(flags.tsconfig),
			kind: 'input',
			field: 'tsconfig',
		},
		{
			label: 'Only',
			description: 'Comma-separated detector IDs or aliases',
			value: valueLabel(flags.only, '(all detectors)'),
			kind: 'input',
			field: 'only',
		},
		{
			label: 'Skip',
			description: 'Comma-separated detector IDs to exclude',
			value: valueLabel(flags.skip, '(none)'),
			kind: 'input',
			field: 'skip',
		},
		{
			label: 'Filter',
			description: 'Kinds or detector categories',
			value: valueLabel(flags.filter, '(all findings)'),
			kind: 'input',
			field: 'filter',
		},
		{
			label: 'Exclude Pattern',
			description: 'Regex applied to export names',
			value: valueLabel(flags.excludePattern, '(none)'),
			kind: 'input',
			field: 'excludePattern',
		},
		{
			label: 'Reporter',
			description: 'Output format when JSON shortcut is off',
			value: flags.reporter ?? 'terminal',
			kind: 'choice',
			field: 'reporter',
			choices: ['terminal', 'json'],
		},
		{
			label: 'JSON Shortcut',
			description: 'Force --json output',
			value: boolLabel(flags.json),
			kind: 'toggle',
			field: 'json',
		},
		{
			label: 'Interactive TUI',
			description: 'Launch findings browser after detection',
			value: boolLabel(flags.interactive),
			kind: 'toggle',
			field: 'interactive',
		},
		{
			label: 'Include UI Dirs',
			description: 'Remove preset UI ignore rules',
			value: boolLabel(flags.includeUi),
			kind: 'toggle',
			field: 'includeUi',
		},
		{
			label: 'Run Detect',
			description: 'Execute with current flags',
			kind: 'action',
			action: 'run',
		},
		{
			label: 'Back',
			description: 'Return to main menu',
			kind: 'action',
			action: 'back',
		},
	]
}

function fixMenuItems(flags: FixFlags): MenuItem[] {
	return [
		{
			label: 'Config Path',
			description: 'Override config discovery',
			value: valueLabel(flags.config),
			kind: 'input',
			field: 'config',
		},
		{
			label: 'Tsconfig Path',
			description: 'Override tsconfig detection',
			value: valueLabel(flags.tsconfig),
			kind: 'input',
			field: 'tsconfig',
		},
		{
			label: 'Only',
			description: 'Comma-separated detector IDs or aliases',
			value: valueLabel(flags.only, '(all detectors)'),
			kind: 'input',
			field: 'only',
		},
		{
			label: 'Skip',
			description: 'Comma-separated detector IDs to exclude',
			value: valueLabel(flags.skip, '(none)'),
			kind: 'input',
			field: 'skip',
		},
		{
			label: 'Mode',
			description: 'Preview or apply changes',
			value: flags.write ? 'write' : 'dry-run',
			kind: 'choice',
			field: 'mode',
			choices: ['dry-run', 'write'],
		},
		{
			label: 'No Backup',
			description: 'Skip snapshots when writing',
			value: boolLabel(flags.noBackup),
			kind: 'toggle',
			field: 'noBackup',
		},
		{
			label: 'Include UI Dirs',
			description: 'Remove preset UI ignore rules',
			value: boolLabel(flags.includeUi),
			kind: 'toggle',
			field: 'includeUi',
		},
		{
			label: 'Run Fix',
			description: 'Execute with current flags',
			kind: 'action',
			action: 'run',
		},
		{
			label: 'Back',
			description: 'Return to main menu',
			kind: 'action',
			action: 'back',
		},
	]
}

function restoreMenuItems(flags: RestoreFlags): MenuItem[] {
	return [
		{
			label: 'Config Path',
			description: 'Override config discovery',
			value: valueLabel(flags.config),
			kind: 'input',
			field: 'config',
		},
		{
			label: 'Tsconfig Path',
			description: 'Override tsconfig detection',
			value: valueLabel(flags.tsconfig),
			kind: 'input',
			field: 'tsconfig',
		},
		{
			label: 'Snapshot',
			description: 'Snapshot id or manifest path',
			value: valueLabel(flags.snapshot, '(required)'),
			kind: 'input',
			field: 'snapshot',
		},
		{
			label: 'Run Restore',
			description: 'Execute with current flags',
			kind: 'action',
			action: 'run',
		},
		{
			label: 'Back',
			description: 'Return to main menu',
			kind: 'action',
			action: 'back',
		},
	]
}

function mainMenuItems(): MenuItem[] {
	return [
		{
			label: 'Detect',
			description: 'Configure and run dead-code analysis',
			kind: 'action',
			action: 'detect',
		},
		{
			label: 'Fix',
			description: 'Configure and preview or apply fixes',
			kind: 'action',
			action: 'fix',
		},
		{
			label: 'Restore',
			description: 'Restore from a snapshot',
			kind: 'action',
			action: 'restore',
		},
		{
			label: 'Info',
			description: 'Show loaded configuration',
			kind: 'action',
			action: 'info',
		},
		{
			label: 'Help',
			description: 'Show CLI help text',
			kind: 'action',
			action: 'help',
		},
		{
			label: 'Quit',
			description: 'Exit kuizer',
			kind: 'action',
			action: 'quit',
		},
	]
}

function currentMenuItems(
	menu: MenuName,
	detectFlags: DetectFlags,
	fixFlags: FixFlags,
	restoreFlags: RestoreFlags
): MenuItem[] {
	if (menu === 'main') return mainMenuItems()
	if (menu === 'detect') return detectMenuItems(detectFlags)
	if (menu === 'fix') return fixMenuItems(fixFlags)
	return restoreMenuItems(restoreFlags)
}

function formatOption(item: MenuItem, selected: boolean): string {
	const value = item.value ? ` ${pc.dim('[' + item.value + ']')}` : ''
	const line = `${item.label}${value}  ${item.description}`
	return selected ? pc.bgCyan(pc.black(` ${line} `)) : `  ${line}`
}

async function promptForValue(
	input: NodeJS.ReadStream,
	output: NodeJS.WriteStream,
	label: string,
	current?: string
): Promise<string | undefined> {
	if (typeof input.setRawMode === 'function') {
		input.setRawMode(false)
	}

	const rl = readline.createInterface({ input, output })
	const prompt = `${label} (${pc.dim('blank keeps current, - clears')})${
		current ? ` [${current}]` : ''
	}: `

	try {
		const answer = await new Promise<string>((resolve) => {
			rl.question(prompt, resolve)
		})
		const trimmed = answer.trim()

		if (trimmed === '') {
			return current
		}

		if (trimmed === '-') {
			return undefined
		}

		return trimmed
	} finally {
		rl.close()
		input.resume()
		if (typeof input.setRawMode === 'function') {
			input.setRawMode(true)
		}
	}
}

function cycleChoice(current: string | undefined, choices: readonly string[]): string {
	const index = current ? choices.indexOf(current) : -1
	return choices[(index + 1 + choices.length) % choices.length] ?? choices[0]!
}

function clearScreen(output: NodeJS.WriteStream): void {
	output.write('\x1b[2J\x1b[H')
}

function renderHeader(
	output: NodeJS.WriteStream,
	version: string,
	title: string,
	config: Config
): void {
	clearScreen(output)
	output.write(pc.bold(pc.green(' Kuizer ')) + pc.dim(` v${version} `) + pc.cyan(title) + '\n\n')
	output.write(pc.dim('─'.repeat(72)) + '\n')
	output.write(`Framework: ${config.framework}\n`)
	output.write(`Tsconfig: ${config.tsconfig}\n`)
	output.write(
		`Ignored paths: ${config.ignore.paths.length > 0 ? config.ignore.paths.join(', ') : '(none)'}\n`
	)
	output.write(
		`Ignored files: ${config.ignore.files.length > 0 ? config.ignore.files.join(', ') : '(none)'}\n\n`
	)
}

function renderMainMenu(
	output: NodeJS.WriteStream,
	version: string,
	config: Config,
	selectedIndex: number
): void {
	renderHeader(output, version, 'Main Menu', config)
	output.write(pc.bold('Commands:') + '\n')
	for (const [index, item] of mainMenuItems().entries()) {
		output.write(formatOption(item, index === selectedIndex) + '\n')
	}
	output.write('\n')
	output.write(pc.dim('Use ↑/↓ to move, Enter to select, q to quit.'))
}

function renderCommandMenu(
	output: NodeJS.WriteStream,
	version: string,
	config: Config,
	title: string,
	items: readonly MenuItem[],
	selectedIndex: number,
	preview: string,
	notes: readonly string[]
): void {
	renderHeader(output, version, title, config)
	output.write(pc.bold('Current Command: ') + preview + '\n\n')
	output.write(pc.bold('Options:') + '\n')
	for (const [index, item] of items.entries()) {
		output.write(formatOption(item, index === selectedIndex) + '\n')
	}
	output.write('\n')
	for (const note of notes) {
		output.write(pc.dim(note) + '\n')
	}
}

function waitForKeypress(input: NodeJS.ReadStream): Promise<KeyInput> {
	return new Promise<KeyInput>((resolve) => {
		const handler = (str: string, key: { name?: string; ctrl?: boolean }) => {
			input.off('keypress', handler)
			resolve({
				str,
				...(key?.name !== undefined ? { name: key.name } : {}),
				...(key?.ctrl !== undefined ? { ctrl: key.ctrl } : {}),
			})
		}

		input.on('keypress', handler)
	})
}

async function waitForContinue(
	input: NodeJS.ReadStream,
	output: NodeJS.WriteStream
): Promise<void> {
	output.write('\n' + pc.dim('Press any key to continue...'))
	await waitForKeypress(input)
}

export async function runMainMenu(options: MainMenuOptions = {}): Promise<number> {
	const input = options.input ?? process.stdin
	const output = options.output ?? process.stdout
	const version = options.version ?? '0.0.0'
	const config = options.config ?? (await loadConfig(process.cwd()))

	if (!input.isTTY || !output.isTTY || typeof input.setRawMode !== 'function') {
		console.log('kuizer: Interactive menu requires a TTY. Use --help for usage.')
		return 1
	}

	let running = true
	let currentMenu: MenuName = 'main'
	let mainIndex = 0
	let detectIndex = 0
	let fixIndex = 0
	let restoreIndex = 0
	let detectFlags = emptyDetectFlags()
	let fixFlags = emptyFixFlags()
	let restoreFlags = emptyRestoreFlags()

	readline.emitKeypressEvents(input)
	input.setRawMode(true)
	input.resume()

	try {
		while (running) {
			if (currentMenu === 'main') {
				renderMainMenu(output, version, config, mainIndex)
			} else if (currentMenu === 'detect') {
				renderCommandMenu(
					output,
					version,
					config,
					'Detect',
					detectMenuItems(detectFlags),
					detectIndex,
					detectCommandPreview(detectFlags),
					[
						'Detector aliases: files, imports, exports, components.',
						'Use Enter to edit or toggle. b/Esc goes back. q quits.',
					]
				)
			} else if (currentMenu === 'fix') {
				renderCommandMenu(
					output,
					version,
					config,
					'Fix',
					fixMenuItems(fixFlags),
					fixIndex,
					fixCommandPreview(fixFlags),
					[
						'Write mode applies changes. Dry-run only previews.',
						'Use Enter to edit or toggle. b/Esc goes back. q quits.',
					]
				)
			} else {
				renderCommandMenu(
					output,
					version,
					config,
					'Restore',
					restoreMenuItems(restoreFlags),
					restoreIndex,
					restoreCommandPreview(restoreFlags),
					[
						'Snapshot accepts either an id or a manifest.json path.',
						'Use Enter to edit. b/Esc goes back. q quits.',
					]
				)
			}

			const key = await waitForKeypress(input)

			if (key.ctrl && key.name === 'c') {
				running = false
				continue
			}

			if (key.str === 'q') {
				running = false
				continue
			}

			if (key.name === 'escape' || key.str === 'b') {
				if (currentMenu === 'main') {
					running = false
				} else {
					currentMenu = 'main'
				}
				continue
			}

			const items = currentMenuItems(currentMenu, detectFlags, fixFlags, restoreFlags)
			const currentIndex =
				currentMenu === 'main'
					? mainIndex
					: currentMenu === 'detect'
						? detectIndex
						: currentMenu === 'fix'
							? fixIndex
							: restoreIndex

			if (key.name === 'up' || key.name === 'down') {
				const delta = key.name === 'up' ? -1 : 1
				const nextIndex = (currentIndex + delta + items.length) % items.length
				if (currentMenu === 'main') mainIndex = nextIndex
				else if (currentMenu === 'detect') detectIndex = nextIndex
				else if (currentMenu === 'fix') fixIndex = nextIndex
				else restoreIndex = nextIndex
				continue
			}

			if (key.name !== 'return') {
				continue
			}

			const selected = items[currentIndex]
			if (!selected) {
				continue
			}

			if (currentMenu === 'main') {
				switch (selected.action) {
					case 'detect':
						currentMenu = 'detect'
						break
					case 'fix':
						currentMenu = 'fix'
						break
					case 'restore':
						currentMenu = 'restore'
						break
					case 'info':
						clearScreen(output)
						await runInfo()
						await waitForContinue(input, output)
						break
					case 'help': {
						const { printMainHelp } = await import('./help.js')
						clearScreen(output)
						printMainHelp(version)
						await waitForContinue(input, output)
						break
					}
					case 'quit':
						running = false
						break
				}
				continue
			}

			if (currentMenu === 'detect') {
				if (selected.kind === 'input' && selected.field) {
					const next = await promptForValue(
						input,
						output,
						selected.label,
						detectFlags[selected.field as keyof DetectFlags] as string | undefined
					)
					detectFlags = { ...detectFlags, [selected.field]: next }
				} else if (selected.kind === 'toggle' && selected.field) {
					detectFlags = {
						...detectFlags,
						[selected.field]: !(
							detectFlags[selected.field as keyof DetectFlags] === true
						),
					}
				} else if (selected.kind === 'choice' && selected.field && selected.choices) {
					const currentValue =
						selected.field === 'reporter'
							? (detectFlags.reporter ?? 'terminal')
							: (detectFlags[selected.field as keyof DetectFlags] as
									| string
									| undefined)
					detectFlags = {
						...detectFlags,
						[selected.field]: cycleChoice(currentValue, selected.choices),
					}
				} else if (selected.action === 'run') {
					clearScreen(output)
					await runDetect(detectFlags, version)
					await waitForContinue(input, output)
				} else if (selected.action === 'back') {
					currentMenu = 'main'
				}
				continue
			}

			if (currentMenu === 'fix') {
				if (selected.kind === 'input' && selected.field) {
					const next = await promptForValue(
						input,
						output,
						selected.label,
						fixFlags[selected.field as keyof FixFlags] as string | undefined
					)
					fixFlags = { ...fixFlags, [selected.field]: next }
				} else if (selected.kind === 'toggle' && selected.field) {
					fixFlags = {
						...fixFlags,
						[selected.field]: !(fixFlags[selected.field as keyof FixFlags] === true),
					}
				} else if (selected.field === 'mode') {
					const nextMode = cycleChoice(fixFlags.write ? 'write' : 'dry-run', [
						'dry-run',
						'write',
					])
					fixFlags = {
						...fixFlags,
						dryRun: nextMode === 'dry-run',
						write: nextMode === 'write',
					}
				} else if (selected.action === 'run') {
					clearScreen(output)
					await runFix(fixFlags)
					await waitForContinue(input, output)
				} else if (selected.action === 'back') {
					currentMenu = 'main'
				}
				continue
			}

			if (selected.kind === 'input' && selected.field) {
				const next = await promptForValue(
					input,
					output,
					selected.label,
					restoreFlags[selected.field as keyof RestoreFlags] as string | undefined
				)
				restoreFlags = { ...restoreFlags, [selected.field]: next }
			} else if (selected.action === 'run') {
				clearScreen(output)
				await runRestore(restoreFlags)
				await waitForContinue(input, output)
			} else if (selected.action === 'back') {
				currentMenu = 'main'
			}
		}
	} finally {
		input.setRawMode(false)
		input.pause()
	}

	return 0
}
