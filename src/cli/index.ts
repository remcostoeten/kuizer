#!/usr/bin/env node

import { fileURLToPath } from 'node:url'
import { main, CliError } from './dispatch.js'

export { main, CliError, runDetect, runFix, runRestore, runInit, runRevert } from './dispatch.js'
export type { DetectFlags, FixFlags, RestoreFlags, InitFlags, RevertFlags } from './parser.js'
export {
	normalizeArgv,
	collectFlagValidationErrors,
	resolveReporter,
	resolveDetectorIds,
	resolveFilterSelection,
	applyFindingFilters,
	compileExcludePattern,
	applyExcludePattern,
	parseCsv,
	resolveDetectorToken,
	DETECTOR_ALIAS_MAP,
	DETECT_FLAGS,
	FIX_FLAGS,
	FLAG_ALIASES,
	FLAG_TO_COMMAND,
	DEFAULT_DETECTOR_IDS,
	SPLIT_EXPORT_DETECTOR_IDS,
} from './parser.js'

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
	try {
		process.exitCode = await main()
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error)
		console.error(`kuizer: ${message}`)
		process.exitCode = 2
	}
}
