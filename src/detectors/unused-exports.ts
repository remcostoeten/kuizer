import { SyntaxKind } from 'ts-morph'
import type { Detector } from './types.js'
import type { Finding, ExportKind, Severity } from '../types.js'

type UnusedExportDetectorId =
	| 'unusedExports'
	| 'unusedFunctions'
	| 'unusedTypes'
	| 'unusedClasses'
	| 'unusedComponents'
	| 'unusedConstants'
	| 'unusedEnums'
	| 'unusedDefaults'
	| 'unusedOthers'

type ExportedNode = {
	getKindName(): string
	getSourceFile(): {
		getFilePath(): string
		getLineAndColumnAtPos(pos: number): { line: number; column: number }
	}
	getStart(): number
}

type ExportFinding = Finding & Readonly<{ detectorId: UnusedExportDetectorId; symbol: string }>

type SourceFileWithExports = {
	getFilePath(): string
	getExportedDeclarations(): ReadonlyMap<string, ExportedNode[]>
	getExportSymbols(): Array<{
		getName(): string
		getAliasedSymbol(): { getName(): string } | undefined
	}>
}

function isLocallyReexported(sourceFile: any, symbol: string): boolean {
	if (!sourceFile || typeof sourceFile.getExportDeclarations !== 'function') {
		return false
	}

	for (const decl of sourceFile.getExportDeclarations()) {
		if (typeof decl.getModuleSpecifierValue === 'function' && decl.getModuleSpecifierValue()) {
			continue
		}

		for (const specifier of decl.getNamedExports()) {
			const exportedName =
				typeof specifier.getAliasNode === 'function' && specifier.getAliasNode()
					? specifier.getAliasNode().getText()
					: specifier.getName()
			if (exportedName === symbol) {
				return true
			}
		}
	}

	return false
}

function getExportKind(sourceFile: any, symbol: string, node: any): ExportKind {
	if (symbol === 'default') return 'default'

	if (!node || !('getKindName' in node)) return 'other'
	if (isLocallyReexported(sourceFile, symbol)) {
		return 'other'
	}

	const kind = node.getKindName()

	if (kind === 'FunctionDeclaration') return 'function'
	if (
		kind === 'VariableStatement' ||
		kind === 'VariableDeclaration' ||
		kind === 'VariableDeclarationList'
	) {
		return 'const'
	}
	if (kind === 'ClassDeclaration') return 'class'
	if (kind === 'InterfaceDeclaration') return 'interface'
	if (kind === 'TypeAliasDeclaration' || kind === 'TSInterfaceDeclaration') return 'type'
	if (kind === 'EnumDeclaration') return 'enum'
	if (kind === 'ExportAssignment') return 'default'

	return 'other'
}

function getSeverityForDetector(
	config: Parameters<Detector>[1],
	detectorId: UnusedExportDetectorId
): Severity {
	return (
		config.detectors[detectorId]?.severity ?? config.detectors.unusedExports?.severity ?? 'warn'
	)
}

function isPascalCase(value: string): boolean {
	return /^[A-Z][A-Za-z0-9]*$/.test(value)
}

function isComponentLike(finding: Pick<Finding, 'file' | 'symbol' | 'kind'>): boolean {
	if (!finding.symbol || !isPascalCase(finding.symbol)) {
		return false
	}

	if (finding.kind !== 'function' && finding.kind !== 'class') {
		return false
	}

	return /\.(jsx|tsx)$/.test(finding.file)
}

function mapDetectorId(finding: Pick<Finding, 'file' | 'symbol' | 'kind'>): UnusedExportDetectorId {
	if (isComponentLike(finding)) {
		return 'unusedComponents'
	}

	switch (finding.kind) {
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

function isFrameworkConsumedExport(
	framework: Parameters<Detector>[1]['framework'],
	filePath: string,
	symbol: string
): boolean {
	if (/\/\.next\//.test(filePath)) return true

	if (symbol === 'default') {
		const rootConfigPattern =
			/\/(next|drizzle|vitest|vite|postcss|tailwind|jest)\.config\.[cm]?[jt]sx?$/
		if (rootConfigPattern.test(filePath)) return true
	}

	const appDirPattern = framework === 'nextjs-app'
	const pagesDirPattern = framework === 'nextjs-pages'

	if (!appDirPattern && !pagesDirPattern) return false

	const isDefault = symbol === 'default'
	const isMetadata = symbol === 'metadata'
	const isGenerateStaticParams = symbol === 'generateStaticParams'
	const isGenerateMetadata = symbol === 'generateMetadata'
	const isRouteHandler =
		symbol === 'GET' ||
		symbol === 'POST' ||
		symbol === 'PUT' ||
		symbol === 'DELETE' ||
		symbol === 'PATCH' ||
		symbol === 'HEAD' ||
		symbol === 'OPTIONS'
	const isRouteSegmentConfig =
		symbol === 'dynamic' ||
		symbol === 'revalidate' ||
		symbol === 'maxDuration' ||
		symbol === 'runtime' ||
		symbol === 'preferredRegion' ||
		symbol === 'fetchCache'
	const isOgImageMeta = symbol === 'alt' || symbol === 'size' || symbol === 'contentType'

	if (pagesDirPattern) {
		if (isDefault && /\/pages\/(?!api\/).+\.(ts|tsx)$/.test(filePath)) return true
		return false
	}

	if (appDirPattern) {
		const isInAppDir = /\/app\//.test(filePath)

		if (isDefault) {
			const defaultExportPattern =
				/\/app\/.*\/(page|layout|loading|error|not-found|template|default)\.(tsx|ts)$/
			const specialFiles = /\/app\/(sitemap|manifest|robots|not-found)\.(tsx|ts)$/
			const ogFiles = /\/app\/.*\/(opengraph|twitter)-image\.tsx$/

			return (
				defaultExportPattern.test(filePath) ||
				specialFiles.test(filePath) ||
				ogFiles.test(filePath)
			)
		}

		if (isMetadata && isInAppDir && /\/(page|layout)\.(tsx|ts)$/.test(filePath)) return true

		if (
			(isGenerateStaticParams || isGenerateMetadata) &&
			/\/app\/.*\/page\.(tsx|ts)$/.test(filePath)
		) {
			return true
		}

		if (isRouteHandler && /\/app\/.*\/route\.(ts|tsx)$/.test(filePath)) return true

		if (
			isRouteSegmentConfig &&
			isInAppDir &&
			/\/(page|layout|route)\.(ts|tsx)$/.test(filePath)
		) {
			return true
		}

		if (isOgImageMeta && /\/app\/.*\/(opengraph|twitter)-image\.tsx$/.test(filePath))
			return true
	}

	return false
}

function createExportKey(symbol: string, node: ExportedNode): string {
	return `${node.getSourceFile().getFilePath()}:${node.getStart()}:${symbol}`
}

function resolveConsumerKey(sourceFile: SourceFileWithExports, symbol: string): string | undefined {
	const nodes = sourceFile.getExportedDeclarations().get(symbol)
	const node = nodes?.[0]
	if (!node) return undefined

	if (node.getSourceFile().getFilePath() === sourceFile.getFilePath()) {
		return createExportKey(symbol, node)
	}

	const exportSymbol = sourceFile.getExportSymbols().find((entry) => entry.getName() === symbol)
	const resolvedSymbol = exportSymbol?.getAliasedSymbol()?.getName() ?? symbol
	return createExportKey(resolvedSymbol, node)
}

function buildConsumerCounts(
	project: Parameters<Detector>[0]['project'],
	reachable: Parameters<Detector>[0]['reachable']
): ReadonlyMap<string, number> {
	const counts = new Map<string, number>()

	const recordConsumer = (sourceFile: SourceFileWithExports, symbol: string) => {
		const key = resolveConsumerKey(sourceFile, symbol)
		if (!key) return

		counts.set(key, (counts.get(key) ?? 0) + 1)
	}

	for (const consumerPath of reachable) {
		const sourceFile = project.getSourceFile(consumerPath)
		if (!sourceFile) continue

		const namespaceImports = new Map<string, SourceFileWithExports>()

		for (const decl of sourceFile.getImportDeclarations()) {
			const importedFile = decl.getModuleSpecifierSourceFile()
			if (!importedFile) continue

			if (decl.getImportClause()?.isTypeOnly()) {
				const defaultImport = decl.getDefaultImport()
				if (defaultImport) {
					recordConsumer(importedFile, 'default')
				}

				for (const specifier of decl.getNamedImports()) {
					recordConsumer(importedFile, specifier.getName())
				}
				continue
			}

			const defaultImport = decl.getDefaultImport()
			if (defaultImport) {
				recordConsumer(importedFile, 'default')
			}

			for (const specifier of decl.getNamedImports()) {
				recordConsumer(importedFile, specifier.getName())
			}

			const namespaceImport = decl.getNamespaceImport()
			if (namespaceImport) {
				namespaceImports.set(namespaceImport.getText(), importedFile)
			}
		}

		if (namespaceImports.size === 0) {
			continue
		}

		for (const expr of sourceFile.getDescendantsOfKind(SyntaxKind.PropertyAccessExpression)) {
			const importedFile = namespaceImports.get(expr.getExpression().getText())
			if (!importedFile) continue

			recordConsumer(importedFile, expr.getName())
		}
	}

	return counts
}

function collectUnusedExportFindings(
	ctx: Parameters<Detector>[0],
	config: Parameters<Detector>[1]
): ExportFinding[] {
	const { project, reachable, analysisBoundaries } = ctx
	const allowed = new Set(config.allowedExports)
	const ignored = new Set(config.ignore.symbols)
	const findings: ExportFinding[] = []
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
			const detectorId = mapDetectorId({ file: path, symbol, kind })

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

function detectById(detectorId: UnusedExportDetectorId): Detector {
	return (ctx, config) => {
		const findings = collectUnusedExportFindings(ctx, config)
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

export const detect = detectById('unusedExports')
export const detectUnusedFunctions = detectById('unusedFunctions')
export const detectUnusedTypes = detectById('unusedTypes')
export const detectUnusedClasses = detectById('unusedClasses')
export const detectUnusedComponents = detectById('unusedComponents')
export const detectUnusedConstants = detectById('unusedConstants')
export const detectUnusedEnums = detectById('unusedEnums')
export const detectUnusedDefaults = detectById('unusedDefaults')
export const detectUnusedOthers = detectById('unusedOthers')
