import type { Detector } from './types.js'
import {
	detectUnusedClasses as unusedClasses,
	detectUnusedComponents as unusedComponents,
	detectUnusedConstants as unusedConstants,
	detectUnusedDefaults as unusedDefaults,
	detectUnusedEnums as unusedEnums,
	detectUnusedFunctions as unusedFunctions,
	detectUnusedOthers as unusedOthers,
	detectUnusedTypes as unusedTypes,
} from './unused-exports.js'
import { detect as unusedImports } from './unused-imports.js'
import { detect as unusedFiles } from './unused-files.js'

export const registry = {
	unusedFiles,
	unusedImports,
	unusedFunctions,
	unusedTypes,
	unusedClasses,
	unusedComponents,
	unusedConstants,
	unusedEnums,
	unusedDefaults,
	unusedOthers,
} as const satisfies Record<string, Detector>
