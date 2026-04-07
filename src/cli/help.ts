import pc from 'picocolors'

interface FlagDef {
	name: string
	short?: string
	aliases: string[]
	type: 'string' | 'boolean'
	placeholder?: string
	description: string
	detail?: string
	defaultValue?: string
}

interface CommandDef {
	name: string
	short?: string
	aliases: string[]
	description: string
	detail?: string
	flags: FlagDef[]
}

const FLAG_CONFIG: FlagDef = {
	name: 'config',
	short: 'c',
	aliases: ['config', '-c', '--config'],
	type: 'string',
	placeholder: '<path>',
	description: 'Custom configuration file',
	detail: 'Override where kuizer looks for configuration.\n\nBy default kuizer searches project config files first, then falls back to ~/.kuizerrc, then the "kuizer" key in package.json.\n\nUseful for:\n  - Project-specific configs (for example configs/frontend.json)\n  - CI pipelines\n  - Monorepos with multiple kuizer setups\n\nExamples:\n  kuizer detect -c configs/frontend.json\n  kuizer fix --config ./.kuizerrc\n  kuizer -c ./custom-kuizer.json',
	defaultValue: 'kuizer.config.json (auto-detected)',
}

const FLAG_TSCONFIG: FlagDef = {
	name: 'tsconfig',
	aliases: ['tsconfig', '--tsconfig'],
	type: 'string',
	placeholder: '<path>',
	description: 'Override TypeScript project config',
	detail: 'Use a specific tsconfig.json instead of the detected one.\n\nWhen you do not pass this flag, kuizer first respects any tsconfig path from config files, then looks for the nearest tsconfig.json from the current working directory upward.\n\nExamples:\n  kuizer detect --tsconfig tsconfig.build.json\n  kuizer fix --tsconfig ./packages/lib/tsconfig.json\n  kuizer --tsconfig tsconfig.esm.json',
	defaultValue: 'nearest tsconfig.json (or config value)',
}

const FLAG_ONLY: FlagDef = {
	name: 'only',
	short: 'o',
	aliases: ['only', '-o', '--only'],
	type: 'string',
	placeholder: '<detectors>',
	description: 'Run specific detectors only',
	detail: 'Run only specific analysis detectors, skipping all others.\n\nAvailable detector IDs:\n  • unusedFiles       - Unreachable files\n  • unusedImports     - Imported bindings with no local uses\n  • unusedFunctions   - Unused function exports\n  • unusedTypes       - Unused type/interface exports\n  • unusedClasses     - Unused class exports\n  • unusedComponents  - Unused component-like exports in TSX/JSX\n  • unusedConstants   - Unused const/variable exports\n  • unusedEnums       - Unused enum exports\n  • unusedDefaults    - Unused default exports\n  • unusedOthers      - Unused uncategorized exports\n\nShorthand aliases:\n  • exports           - All split export detectors\n  • files             - unusedFiles\n  • imports           - unusedImports\n\nExamples:\n  kuizer detect --only unusedImports\n  kuizer detect --only unusedComponents\n  kuizer fix --only exports\n  kuizer fix --only unusedFiles,unusedFunctions',
}

const FLAG_SKIP: FlagDef = {
	name: 'skip',
	short: 's',
	aliases: ['skip', '-s', '--skip'],
	type: 'string',
	placeholder: '<detectors>',
	description: 'Skip specific detectors',
	detail: 'Exclude specific detectors from the analysis run.\n\nUse the same detector IDs as --only.\n\nCommon scenarios:\n  • Skip unusedFiles in repos with fixture folders or generated packages\n  • Skip unusedImports unless you specifically want import hygiene\n  • Skip one export category while focusing on another\n\nExamples:\n  kuizer detect --skip unusedFiles\n  kuizer detect --skip unusedImports\n  kuizer fix --skip unusedComponents,unusedClasses\n  kuizer fix --skip exports',
}

// ── Detect flags ────────────────────────────────────────────────────

const FLAG_REPORTER: FlagDef = {
	name: 'reporter',
	short: 'r',
	aliases: ['reporter', 'report', '-r', '--reporter', '--report'],
	type: 'string',
	placeholder: '<format>',
	description: 'Output format: terminal or json',
	detail: 'Choose how detect results are displayed.\n\nFormats:\n  • terminal - Colored, human-readable output with file paths and line numbers (default)\n  • json     - Machine-readable JSON for scripts and CI\n\nExamples:\n  kuizer detect --reporter json | jq .\n  kuizer -r terminal',
	defaultValue: 'terminal',
}

const FLAG_JSON: FlagDef = {
	name: 'json',
	aliases: ['--json', 'json'],
	type: 'boolean',
	description: 'Output results as JSON',
	detail: 'Shortcut for --reporter json.\n\nPrints all findings as structured JSON to stdout, perfect for:\n  - CI/CD pipelines and automation\n  - Parsing in scripts (jq, grep, etc.)\n  - Sharing results (json files, APIs)\n  - Custom reporting tools\n\nThe JSON includes:\n  • Metadata (version, tsconfig, timestamp)\n  • Complete file list with import counts\n  • All findings with source locations\n\nCan be used as:\n  • kuizer detect --json\n  • kuizer d --json\n  • kuizer d json\n  • kuizer json\n  • kuizer --json\n\nExamples:\n  kuizer detect --json | jq \'.findings | length\'\n  kuizer d --json > findings.json\n  kuizer json | grep -o "path"',
}

const FLAG_FILTER: FlagDef = {
	name: 'filter',
	short: 'f',
	aliases: ['filter', '-f', '--filter'],
	type: 'string',
	placeholder: '<types>',
	description: 'Filter by export type',
	detail: 'Include only specific finding kinds or detector categories in detect results.\n\nKinds:\n  • functions   - Function declarations and expressions\n  • types       - Type aliases\n  • interfaces  - Interface declarations\n  • classes     - Class declarations\n  • vars        - Variables and constants\n  • enums       - Enum declarations\n  • defaults    - Default exports\n  • all         - Show everything\n\nDetector categories can also be used here:\n  • unusedImports\n  • unusedComponents\n  • exports      - All split export detectors\n\nExamples:\n  kuizer detect -f functions,types\n  kuizer detect -f unusedComponents\n  kuizer d --filter unusedImports,classes',
}

const FLAG_EXCLUDE_PATTERN: FlagDef = {
	name: 'exclude-pattern',
	aliases: ['--exclude-pattern'],
	type: 'string',
	placeholder: '<regex>',
	description: 'Exclude symbols matching regex',
	detail: 'Ignore exports whose names match a JavaScript regular expression.\n\nThe pattern is tested against the export name only, not the file path.\n\nExamples:\n  kuizer detect --exclude-pattern "^_"\n  kuizer d --exclude-pattern "^__.*__$"',
}

const FLAG_INTERACTIVE: FlagDef = {
	name: 'interactive',
	aliases: ['--interactive'],
	type: 'boolean',
	description: 'Launch interactive TUI',
	detail: 'Open a terminal UI to browse findings and apply supported fixes.\n\nFeatures:\n  • File and finding panes\n  • Kind filters and detector-category filters\n  • Selection and batch apply actions\n  • In-session revert of the last applied snapshot\n\nKeyboard shortcuts:\n  • ↑/↓         - Navigate\n  • Tab         - Switch pane\n  • Space       - Select current item\n  • Enter       - Apply the current item or file\n  • x           - Apply selected items\n  • A           - Apply all filtered items\n  • f/c/t/i     - Toggle function, const, type, interface filters\n  • 1-9,0       - Toggle detector categories shown in the header\n  • a           - Show all kinds\n  • *           - Show all detector categories\n  • r           - Reset kind and detector filters\n  • R           - Revert the last applied snapshot\n  • q           - Quit\n\nExample:\n  kuizer detect --interactive --only unusedComponents',
}

// ── Fix flags ───────────────────────────────────────────────────────

const FLAG_DRY_RUN: FlagDef = {
	name: 'dry-run',
	aliases: ['dry', '--dry', '--dr', '--dry-run'],
	type: 'boolean',
	description: 'Preview fixes without writing',
	detail: 'Show exactly what would be changed without touching files.\n\nDisplays:\n  • Unified diff format (like git diff)\n  • Added/removed lines with context\n  • File paths and line numbers\n  • Summary of changes\n\nSafe to run anytime:\n  - Before committing changes\n  - In CI pipelines to verify impact\n  - When unsure about a fix\n  - To review automated changes\n\nThe diff uses standard format so you can pipe it:\n  kuizer fix --dry | colordiff\n  kuizer f --dry > review.patch\n\nExamples:\n  kuizer fix --dry\n  kuizer f --dry --only unusedExports\n  kuizer dry --skip unusedFiles',
}

const FLAG_WRITE: FlagDef = {
	name: 'write',
	short: 'w',
	aliases: ['write', '-w', '--write'],
	type: 'boolean',
	description: 'Apply fixes to files',
	detail: 'Actually modify files based on the detected unused code.\n\nWhat happens:\n  • Removes supported unused export declarations\n  • Deletes entirely unused files\n  • Creates a backup snapshot (unless --no-backup)\n\nSafety features:\n  - Automatic backup before any changes\n  - Only applies supported planner actions\n  - Preserves the current file formatting as written\n\nRequired for actual cleanup; --dry-run only previews.\n\nExamples:\n  kuizer fix --write\n  kuizer f -w --only unusedExports\n  kuizer fix -w --dry-run  # error: mutually exclusive',
}

const FLAG_NO_BACKUP: FlagDef = {
	name: 'no-backup',
	aliases: ['no-backup', '--no-backup'],
	type: 'boolean',
	description: 'Skip backup before applying fixes',
	detail: "Disable the automatic snapshot creation when using --write.\n\nBy default, kuizer creates a .kuizer/snapshots/<id>/ directory\ncontaining:\n  • All modified files (original versions)\n  • A manifest.json with metadata\n  • Restoration command for rollback\n\nUse --no-backup when:\n  - Running in CI/CD (ephemeral environments)\n  - Using version control (git reset --hard)\n  - Disk space is constrained\n  - You explicitly don't need rollback capability\n\n⚠️  Without backups, you cannot use kuizer restore\n    to undo changes. Use with caution.\n\nExamples:\n  kuizer fix --write --no-backup\n  kuizer f -w --no-backup --only unusedFiles\n  kuizer fix -w --no-backup --dry-run  # error",
}

// ── Restore flags ───────────────────────────────────────────────────

const FLAG_SNAPSHOT: FlagDef = {
	name: 'snapshot',
	aliases: ['--snapshot'],
	type: 'string',
	placeholder: '<path|id>',
	description: 'Snapshot to restore from',
	detail: 'Restore files to a previous state using a backup snapshot.\n\nPass either a snapshot ID or a manifest.json path.\n\nExamples:\n  kuizer restore --snapshot 2026-04-05T04-45-49-701Z-fix-f1f310e4\n  kuizer restore --snapshot ./.kuizer/snapshots/<id>/manifest.json',
}

// ── Command definitions ─────────────────────────────────────────────

const CMD_DETECT: CommandDef = {
	name: 'detect',
	short: 'd',
	aliases: ['detect', 'd', '--detect'],
	description: 'Analyze your codebase for dead code',
	detail: 'Scans your TypeScript project using the configured tsconfig, builds an import graph, and reports unused exports and unreachable files.',
	flags: [
		FLAG_CONFIG,
		FLAG_TSCONFIG,
		FLAG_ONLY,
		FLAG_SKIP,
		FLAG_REPORTER,
		FLAG_JSON,
		FLAG_FILTER,
		FLAG_EXCLUDE_PATTERN,
		FLAG_INTERACTIVE,
	],
}

const CMD_FIX: CommandDef = {
	name: 'fix',
	short: 'f',
	aliases: ['fix', 'f', '--fix'],
	description: 'Remove unused exports and dead code',
	detail: 'Plans and optionally applies automated fixes. Use --dry-run to preview, --write to apply. Creates a backup snapshot by default.',
	flags: [
		FLAG_CONFIG,
		FLAG_TSCONFIG,
		FLAG_ONLY,
		FLAG_SKIP,
		FLAG_DRY_RUN,
		FLAG_WRITE,
		FLAG_NO_BACKUP,
	],
}

const CMD_RESTORE: CommandDef = {
	name: 'restore',
	short: 'r',
	aliases: ['restore', 'r', '--restore'],
	description: 'Restore files from a backup snapshot',
	detail: 'Reverts files to a previous state using a snapshot created by the fix command. Requires a --snapshot path or ID.',
	flags: [FLAG_CONFIG, FLAG_SNAPSHOT],
}

const CMD_INIT: CommandDef = {
	name: 'init',
	aliases: ['init', '--init'],
	description: 'Create a kuizer configuration file',
	detail: 'Runs the interactive configuration setup and writes either a project-local config or a global ~/.kuizerrc file.',
	flags: [],
}

const CMD_REVERT: CommandDef = {
	name: 'revert',
	short: 'rv',
	aliases: ['revert', 'rv', '--revert'],
	description: 'Revert changes from a snapshot',
	detail: 'Quickly revert the last applied fixes using a snapshot ID or path. This is a shortcut for the restore command focused on undoing recent changes.',
	flags: [FLAG_CONFIG, FLAG_SNAPSHOT],
}

const COMMANDS: CommandDef[] = [CMD_DETECT, CMD_FIX, CMD_RESTORE, CMD_INIT, CMD_REVERT]

const ALL_FLAGS: FlagDef[] = [
	FLAG_CONFIG,
	FLAG_TSCONFIG,
	FLAG_ONLY,
	FLAG_SKIP,
	FLAG_REPORTER,
	FLAG_JSON,
	FLAG_FILTER,
	FLAG_EXCLUDE_PATTERN,
	FLAG_INTERACTIVE,
	FLAG_DRY_RUN,
	FLAG_WRITE,
	FLAG_NO_BACKUP,
	FLAG_SNAPSHOT,
]

const ANSI_RE = new RegExp(String.raw`\u001B\[[0-9;]*m`, 'g')
function visible(s: string): number {
	return s.replace(ANSI_RE, '').length
}

function padTo(s: string, col: number): string {
	const gap = Math.max(1, col - visible(s))
	return s + ' '.repeat(gap)
}

const DESC_COL = 34

function formatFlagLine(flag: FlagDef): string {
	let left = '  '
	if (flag.short) {
		left += pc.cyan(`-${flag.short}`) + pc.dim(',') + ' '
	} else {
		left += '    '
	}
	left += pc.cyan(`--${flag.name}`)
	if (flag.placeholder) {
		left += ' ' + pc.dim(flag.placeholder)
	}

	const aliases = flag.aliases.filter((a) => a !== `--${flag.name}` && a !== `-${flag.short}`)
	let right = flag.description
	if (flag.defaultValue) {
		right += pc.dim(` [${flag.defaultValue}]`)
	}
	if (aliases.length > 0) {
		right += pc.dim(`  (also: ${aliases.join(', ')})`)
	}
	return padTo(left, DESC_COL) + right
}

// Command column for the COMMANDS block
const CMD_DESC_COL = 34

function formatCommandLine(cmd: CommandDef): string {
	const name = pc.green(pc.bold(cmd.name))
	const rest = [cmd.short, ...cmd.aliases.filter((a) => a !== cmd.name && a !== cmd.short)]
		.filter(Boolean)
		.map((a) => pc.dim(a!))
		.join(pc.dim(' | '))
	const left = `  ${name} ${pc.dim('|')} ${rest}`
	return padTo(left, CMD_DESC_COL) + cmd.description
}

// ── Main help ───────────────────────────────────────────────────────

export function printMainHelp(version: string): void {
	const o: string[] = []
	const d = pc.dim
	const b = pc.bold
	const g = pc.green
	const c = pc.cyan
	const w = pc.white

	o.push('')
	o.push(
		`  ${b(g('kuizer'))} ${d(`v${version}`)}  ${d('TypeScript dead code intelligence for large codebases.')}`
	)
	o.push('')
	o.push(
		`  ${b(w('USAGE'))}   ${g('kuizer')} ${c('<command>')} ${d('[flags]')}     ${g('kuizer')} ${c('<flag>')} ${d('— auto-detects command')}`
	)
	o.push('')

	// Commands
	o.push(`  ${b(w('COMMANDS'))}`)
	o.push(formatCommandLine(CMD_DETECT))
	o.push(formatCommandLine(CMD_FIX))
	o.push(formatCommandLine(CMD_RESTORE))
	o.push(formatCommandLine(CMD_INIT))
	o.push(formatCommandLine(CMD_REVERT))
	o.push(`  ${b(w('META'))}`)
	o.push(
		padTo(`  ${g(b('help'))} ${d('| h | -h | --help')}`, CMD_DESC_COL) + 'Show this help menu'
	)
	o.push(
		padTo(`  ${g(b('version'))} ${d('| v | -v | --version')}`, CMD_DESC_COL) + 'Show version'
	)
	o.push('')

	// Detect flags
	o.push(`  ${b(w('DETECT FLAGS'))} ${d('— detect | d | --detect')}`)
	for (const flag of CMD_DETECT.flags) {
		o.push(formatFlagLine(flag))
	}
	o.push('')

	// Fix flags
	o.push(`  ${b(w('FIX FLAGS'))} ${d('— fix | f | --fix')}`)
	for (const flag of CMD_FIX.flags) {
		o.push(formatFlagLine(flag))
	}
	o.push('')

	// Restore flags
	o.push(`  ${b(w('RESTORE FLAGS'))} ${d('— restore | r | --restore')}`)
	for (const flag of CMD_RESTORE.flags) {
		o.push(formatFlagLine(flag))
	}
	o.push('')

	// Examples in two columns
	const EX_CMD = 32
	const EX_HALF = 56
	const exL: [string, string][] = [
		['kuizer detect', 'Run all detectors'],
		['kuizer d -o unusedExports', 'Only unused exports'],
		['kuizer d json', 'Output as JSON'],
		['kuizer json', 'JSON output (standalone)'],
		['kuizer d -f functions,types', 'Filter by type'],
	]
	const exR: [string, string][] = [
		['kuizer fix --dry', 'Preview fixes'],
		['kuizer f -w', 'Apply fixes'],
		['kuizer f -w --no-backup', 'Skip backup'],
		['kuizer revert --snapshot <id>', 'Revert changes'],
		['kuizer restore --snapshot <id>', 'Restore'],
		['kuizer dry -h', 'Flag help'],
	]
	o.push(`  ${b(w('EXAMPLES'))}`)
	for (let i = 0; i < exL.length; i++) {
		const l = exL[i]!
		const r = exR[i]
		const left = padTo(`    ${g(l[0])}`, EX_CMD) + d(l[1])
		if (r) {
			o.push(padTo(left, EX_HALF) + padTo(g(r[0]), EX_CMD) + d(r[1]))
		} else {
			o.push(left)
		}
	}
	o.push('')

	// Combining + flag help in two columns
	o.push(padTo(`  ${b(w('COMBINING'))}`, EX_HALF) + `${b(w('FLAG HELP'))}`)
	o.push(
		padTo(`    ${g('kuizer d -c cfg.json -o unusedExports')}`, EX_HALF) +
			`${d('Add')} ${c('-h')}${d(',')} ${c('--help')} ${d('or')} ${c('help')} ${d('after any flag:')}`
	)
	o.push(
		padTo(`    ${g('kuizer fix --dry --only unusedExports')}`, EX_HALF) +
			`  ${g('kuizer dry -h')}       ${d('→ --dry-run info')}`
	)
	o.push(
		padTo(`    ${g('kuizer f -w --no-backup -c cfg.json')}`, EX_HALF) +
			`  ${g('kuizer config --help')} ${d('→ --config info')}`
	)
	o.push('')

	process.stdout.write(o.join('\n') + '\n')
}

function printCommandHelp(cmdName: string, version: string): void {
	const cmd = COMMANDS.find(
		(c) => c.name === cmdName || c.short === cmdName || c.aliases.includes(cmdName)
	)
	if (!cmd) return

	const o: string[] = []
	const d = pc.dim
	const b = pc.bold
	const g = pc.green
	const c = pc.cyan
	const w = pc.white

	const cmdNames = [
		cmd.name,
		cmd.short,
		...cmd.aliases.filter((a) => a !== cmd.name && a !== cmd.short),
	].filter(Boolean)

	o.push('')
	o.push(`  ${b(g('kuizer ' + cmd.name))} ${d(`v${version}`)}`)
	o.push(`  ${cmd.detail ?? cmd.description}`)
	o.push('')
	o.push(`  ${b(w('USAGE'))}   ${g('kuizer')} ${c(cmdNames.join(d(' | ')))} ${d('[flags]')}`)
	o.push('')
	o.push(`  ${b(w('FLAGS'))}`)
	for (const flag of cmd.flags) {
		o.push(formatFlagLine(flag))
	}
	o.push('')

	process.stdout.write(o.join('\n') + '\n')
}

function printFlagHelp(flagInput: string): void {
	const normalized = flagInput.replace(/^-+/, '').toLowerCase()
	const flag = ALL_FLAGS.find(
		(f) =>
			f.name === normalized ||
			f.short === normalized ||
			f.aliases.some((a) => a.replace(/^-+/, '') === normalized)
	)
	if (!flag) return

	const lines: string[] = []
	const d = pc.dim
	const b = pc.bold
	const g = pc.green
	const c = pc.cyan
	const w = pc.white

	lines.push('')
	lines.push(`  ${b(c('--' + flag.name))} ${flag.placeholder ? d(flag.placeholder) : ''}`)
	lines.push('')

	if (flag.detail) {
		for (const line of flag.detail.split('\n')) {
			lines.push(`  ${line}`)
		}
		lines.push('')
	}

	// Aliases
	const allAliases = [
		flag.short ? `-${flag.short}` : null,
		`--${flag.name}`,
		...flag.aliases.filter((a) => a !== `--${flag.name}` && a !== `-${flag.short}`),
	].filter(Boolean)
	lines.push(`  ${b(w('Aliases:'))}  ${allAliases.map((a) => g(a!)).join(d(', '))}`)

	lines.push(
		`  ${b(w('Type:'))}     ${d(flag.type === 'string' ? `string ${flag.placeholder ?? ''}` : 'boolean')}`
	)

	if (flag.defaultValue) {
		lines.push(`  ${b(w('Default:'))}  ${d(flag.defaultValue)}`)
	}
	const usedIn = COMMANDS.filter((cmd) => cmd.flags.some((f) => f.name === flag.name))
	if (usedIn.length > 0) {
		lines.push(`  ${b(w('Used in:'))}  ${usedIn.map((cmd) => g(cmd.name)).join(d(', '))}`)
	}

	lines.push('')

	process.stdout.write(lines.join('\n') + '\n')
}

function findFlagByInput(input: string): FlagDef | undefined {
	const normalized = input.replace(/^-+/, '').toLowerCase()
	return ALL_FLAGS.find(
		(f) =>
			f.name === normalized ||
			f.short === normalized ||
			f.aliases.some((a) => a.replace(/^-+/, '') === normalized)
	)
}

function isHelpArg(arg: string): boolean {
	return arg === '-h' || arg === '--help' || arg === 'help'
}

/**
 * Try to handle help requests. Returns true if help was printed.
 */
export function tryHandleHelp(argv: string[], version: string): boolean {
	const first = argv[0] ?? ''
	const second = argv[1] ?? ''
	const third = argv[2] ?? ''

	// No args → main help
	if (argv.length === 0) {
		printMainHelp(version)
		return true
	}

	// Single arg: help, -h, --help → main help
	if (argv.length === 1 && isHelpArg(first)) {
		printMainHelp(version)
		return true
	}

	// Single arg: version, v, -v, --version → print version
	const versionAliases = new Set(['version', 'v', '-v', '--version'])
	if (argv.length === 1 && versionAliases.has(first)) {
		console.log(version)
		return true
	}

	// version + help → main help
	if (argv.length === 2 && versionAliases.has(first) && isHelpArg(second)) {
		printMainHelp(version)
		return true
	}

	// Two args where second is a help request
	if (argv.length === 2 && isHelpArg(second)) {
		// Is the first arg a command? → command help
		const cmd = COMMANDS.find(
			(c) => c.name === first || c.short === first || c.aliases.includes(first)
		)
		if (cmd) {
			printCommandHelp(first, version)
			return true
		}
		// Is it a flag? → flag help
		const flag = findFlagByInput(first)
		if (flag) {
			printFlagHelp(first)
			return true
		}
	}

	// Three args: command + flag + help
	// e.g. "detect --config -h", "fix dry --help"
	if (argv.length === 3 && isHelpArg(third)) {
		const flag = findFlagByInput(second)
		if (flag) {
			printFlagHelp(second)
			return true
		}
	}

	// Last arg is help and the second-to-last is a flag
	if (argv.length >= 2) {
		const lastArg = argv[argv.length - 1] ?? ''
		const prevArg = argv[argv.length - 2] ?? ''
		if (isHelpArg(lastArg) && findFlagByInput(prevArg)) {
			printFlagHelp(prevArg)
			return true
		}
	}

	return false
}
