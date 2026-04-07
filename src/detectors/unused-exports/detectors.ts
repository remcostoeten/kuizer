import type { Ctx, Config, Finding, Severity } from '../../types.js'
import type { Detector } from '../types.js'
import { classifyExport, type UnusedExportDetectorId } from './classification.js'
import {
	createExportKey,
	isFrameworkConsumedExport,
	getExportKind,
} from './collector.js'
import { buildConsumerCounts } from './consumer-graph.js'

const EXPORT_DETECTOR_IDS = [
	'unusedExports',
	'unusedFunctions',
	'unusedTypes',
	'unusedClasses',
	'unusedComponents',
	'unusedConstants',
	'unusedEnums',
	'unusedDefaults',
	'unusedOthers',
] as const satisfies readonly UnusedExportDetectorId[]

const EXPORT_FINDING_CACHE = new WeakMap<Ctx, Map<string, readonly Finding[]>>()

function getSeverityForDetector(
	config: Config,
	detectorId: UnusedExportDetectorId
): Severity {
	return (
		config.detectors[detectorId]?.severity ?? config.detectors.unusedExports?.severity ?? 'warn'
	)
}

function collectUnusedExportFindings(ctx: Ctx, config: Config): Finding[] {
	const { project, reachable, analysisBoundaries } = ctx
	const allowed = new Set(config.allowedExports)
	const ignored = new Set(config.ignore.symbols as readonly string[])
	const findings: Finding[] = []
	const hasDynamicImportBoundary = analysisBoundaries.some(
		(boundary) => boundary.kind === 'dynamic-import'
	)
	const consumerCounts = buildConsumerCounts(project, reachable)
	const reported = new Set<string>()

	for (const path of reachable) {
		const sf = project.getSourceFile(path)
		if (!sf) continue

		for (const [symbol, nodes] of sf.getExportedDeclarations()) {
			if (isFrameworkConsumedExport(config.framework, path, symbol)) continue
			if (allowed.has(symbol)) continue
			if (ignored.has(symbol)) continue

			const node = nodes[0]
			if (!node) continue

			if (node.getSourceFile().getFilePath() !== path) {
				continue
			}

			const exportKey = createExportKey(symbol, node)
			if (reported.has(exportKey)) {
				continue
			}
			reported.add(exportKey)

			if ((consumerCounts.get(exportKey) ?? 0) !== 0) {
				continue
			}

			const { line, column } = node.getSourceFile().getLineAndColumnAtPos(node.getStart())
			const kind = getExportKind(sf, symbol, node)
			const detectorId = classifyExport(path, symbol, kind)

			findings.push({
				detectorId,
				severity: getSeverityForDetector(config, detectorId),
				file: path,
				line,
				col: column,
				message: hasDynamicImportBoundary
					? `Exported symbol '${symbol}' has 0 statically resolved consumers`
					: `Exported symbol '${symbol}' has 0 consumers`,
				symbol,
				kind,
				fixable: false,
				context: {
					consumerCount: 0,
					framework: config.framework,
					analysisBoundaryCount: analysisBoundaries.length,
					hasDynamicImportBoundary,
				},
			})
		}
	}

	return findings
}

function buildCacheKey(config: Config): string {
	return JSON.stringify({
		framework: config.framework,
		allowedExports: [...config.allowedExports].sort(),
		ignoreSymbols: [...(config.ignore.symbols as readonly string[])].sort(),
		detectors: Object.fromEntries(
			EXPORT_DETECTOR_IDS.map((detectorId) => [
				detectorId,
				{
					enabled: config.detectors[detectorId]?.enabled ?? null,
					severity: config.detectors[detectorId]?.severity ?? null,
				},
			])
		),
	})
}

function getCachedUnusedExportFindings(ctx: Ctx, config: Config): readonly Finding[] {
	const contextCache = EXPORT_FINDING_CACHE.get(ctx)
	if (!contextCache) {
	const findings = collectUnusedExportFindings(ctx, config)
	const contextCache = new Map<string, readonly Finding[]>([[buildCacheKey(config), findings]])
	EXPORT_FINDING_CACHE.set(ctx, contextCache)
	return findings
	}

	const cacheKey = buildCacheKey(config)
	const cached = contextCache.get(cacheKey)
	if (cached) {
		return cached
	}

	const findings = collectUnusedExportFindings(ctx, config)
	contextCache.set(cacheKey, findings)
	return findings
}

export function createExportDetector(detectorId: UnusedExportDetectorId): Detector {
	return (ctx, config) => {
		const findings = getCachedUnusedExportFindings(ctx, config)
		if (detectorId === 'unusedExports') {
			return findings.map((finding) => ({
				...finding,
				detectorId,
				severity: getSeverityForDetector(config, detectorId),
			}))
		}

		return findings.filter((finding) => finding.detectorId === detectorId)
	}
}

export const detect = createExportDetector('unusedExports')
export const detectUnusedFunctions = createExportDetector('unusedFunctions')
export const detectUnusedTypes = createExportDetector('unusedTypes')
export const detectUnusedClasses = createExportDetector('unusedClasses')
export const detectUnusedComponents = createExportDetector('unusedComponents')
export const detectUnusedConstants = createExportDetector('unusedConstants')
export const detectUnusedEnums = createExportDetector('unusedEnums')
export const detectUnusedDefaults = createExportDetector('unusedDefaults')
export const detectUnusedOthers = createExportDetector('unusedOthers')
