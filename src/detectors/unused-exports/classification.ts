export function isPascalCase(value: string): boolean {
	return /^[A-Z][A-Za-z0-9]*$/.test(value)
}

export function isComponentLike(file: string, symbol: string | undefined, kind: string | undefined): boolean {
	if (!symbol || !isPascalCase(symbol)) {
		return false
	}

	if (kind !== 'function' && kind !== 'class') {
		return false
	}

	return /\.(jsx|tsx)$/.test(file)
}

export type UnusedExportDetectorId =
	| 'unusedExports'
	| 'unusedFunctions'
	| 'unusedTypes'
	| 'unusedClasses'
	| 'unusedComponents'
	| 'unusedConstants'
	| 'unusedEnums'
	| 'unusedDefaults'
	| 'unusedOthers'

export function classifyExport(
	file: string,
	symbol: string | undefined,
	kind: string | undefined
): UnusedExportDetectorId {
	if (isComponentLike(file, symbol, kind)) {
		return 'unusedComponents'
	}

	switch (kind) {
		case 'function':
			return 'unusedFunctions'
		case 'type':
		case 'interface':
			return 'unusedTypes'
		case 'class':
			return 'unusedClasses'
		case 'const':
			return 'unusedConstants'
		case 'enum':
			return 'unusedEnums'
		case 'default':
			return 'unusedDefaults'
		default:
			return 'unusedOthers'
	}
}
