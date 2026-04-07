import { cli, command } from 'cleye'
import {
	normalizeArgv,
	collectFlagValidationErrors,
	type DetectFlags,
	type FixFlags,
	type RestoreFlags,
	type InitFlags,
	type RevertFlags,
	type FlagRecord,
} from './parser.js'
import { getVersion } from '../meta.js'
import { runDetect, runFix, runRestore, runInit, runRevert } from './commands.js'
import { printMainHelp, tryHandleHelp } from './help.js'

export class CliError extends Error {
	exitCode: number

	constructor(message: string, exitCode = 1) {
		super(message)
		this.name = 'CliError'
		this.exitCode = exitCode
	}
}

export async function main(argv: string[] = process.argv.slice(2)): Promise<number> {
	const version = getVersion(import.meta.url)
	const normalizedArgv = normalizeArgv(argv)

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
		return runDetect(parsed.flags as unknown as DetectFlags, version)
	}

	if (parsed.command === 'fix') {
		return runFix(parsed.flags as unknown as FixFlags)
	}

	if (parsed.command === 'restore') {
		return runRestore(parsed.flags as unknown as RestoreFlags)
	}

	if (parsed.command === 'init') {
		return runInit(parsed.flags as unknown as InitFlags)
	}

	if (parsed.command === 'revert') {
		return runRevert(parsed.flags as unknown as RevertFlags)
	}

	printMainHelp(version)
	return 1
}

export { runDetect, runFix, runRestore, runInit, runRevert } from './commands.js'
export type { DetectFlags, FixFlags, RestoreFlags, InitFlags, RevertFlags } from './parser.js'
