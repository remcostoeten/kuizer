import type { FixPlanner } from './types.js'
import { planUnusedExportsFix } from './planners/unused-exports.js'
import { planUnusedFilesFix } from './planners/unused-files.js'

export const registry = {
	unusedExports: planUnusedExportsFix,
	unusedFiles: planUnusedFilesFix,
} as const satisfies Record<string, FixPlanner>
