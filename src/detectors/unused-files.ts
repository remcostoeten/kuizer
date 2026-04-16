import type { Finding } from '../types.js'
import type { Detector } from './types.js'

export function detect(
	...[{ all, reachable, testReachable, analysisBoundaries }, config]: Parameters<Detector>
): ReturnType<Detector> {
	const severity = config.detectors.unusedFiles?.severity ?? 'warn'
	const ignored = new Set(config.ignore.files)
	const hasDynamicImportBoundary = analysisBoundaries.some(
		(boundary) => boundary.kind === 'dynamic-import'
	)
	const findings: Finding[] = []

	for (const file of all) {
		if (reachable.has(file) || ignored.has(file)) {
			continue
		}

		findings.push({
			detectorId: 'unusedFiles',
			severity,
			file,
			line: 1,
			col: 1,
			message: hasDynamicImportBoundary
				? 'File is unreachable from any statically resolved entry point'
				: 'File is unreachable from any entry point',
			fixable: false,
			...(hasDynamicImportBoundary
				? {
						context: {
							analysisBoundaryCount: analysisBoundaries.length,
							hasDynamicImportBoundary: true,
						},
					}
				: {}),
		})
	}

	return findings
}
