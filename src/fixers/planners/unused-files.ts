import type { FixPlanner } from '../types.js'

export const planUnusedFilesFix: FixPlanner = (finding, ctx) => {
	if (finding.detectorId !== 'unusedFiles') {
		return null
	}

	if (!ctx.all.has(finding.file)) {
		return {
			supported: false,
			finding,
			reason: 'Target file is not present in the analyzed project',
		}
	}

	if (ctx.reachable.has(finding.file)) {
		return {
			supported: false,
			finding,
			reason: 'Target file is still reachable in the current analysis graph',
		}
	}

	if (finding.context?.hasDynamicImportBoundary === true) {
		return {
			supported: false,
			finding,
			reason: 'Dynamic import analysis boundaries make file deletion uncertain',
		}
	}

	return {
		supported: true,
		finding,
		edits: [
			{
				kind: 'delete-file',
				file: finding.file,
				description: 'Delete unreachable file',
			},
		],
		safety: 'destructive',
		reason: 'File is unreachable and can be removed as a whole-file deletion',
	}
}
