import type { FixPlanner } from './types.js'
import { planUnusedExportsFix } from './planners/unused-exports.js'
import { planUnusedFilesFix } from './planners/unused-files.js'

export const registry = {
	unusedExports: planUnusedExportsFix,
	unusedFunctions: planUnusedExportsFix,
	unusedTypes: planUnusedExportsFix,
	unusedClasses: planUnusedExportsFix,
	unusedComponents: planUnusedExportsFix,
	unusedConstants: planUnusedExportsFix,
	unusedEnums: planUnusedExportsFix,
	unusedDefaults: planUnusedExportsFix,
	unusedOthers: planUnusedExportsFix,
	unusedFiles: planUnusedFilesFix,
} as const satisfies Record<string, FixPlanner>
