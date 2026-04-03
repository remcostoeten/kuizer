import { SyntaxKind } from 'ts-morph'
import type { Detector } from './types.js'
import type { Finding, ExportKind } from '../types.js'

function getExportKind(node: any): ExportKind {
	if (!node || !('getKindName' in node)) return 'other'

	const kind = node.getKindName()

	if (kind === 'FunctionDeclaration') return 'function'
	if (kind === 'VariableStatement') return 'const'
	if (kind === 'ClassDeclaration') return 'class'
	if (kind === 'InterfaceDeclaration') return 'interface'
	if (kind === 'TypeAliasDeclaration' || kind === 'TSInterfaceDeclaration') return 'type'
	if (kind === 'EnumDeclaration') return 'enum'
	if (kind === 'ExportAssignment') return 'default'

	return 'other'
}

function countsAsConsumer(refEntry: {
	getNode(): { getKindName(): string; getParent(): any }
}): boolean {
	let current: { getKindName(): string; getParent(): any } | undefined = refEntry.getNode()

	while (current) {
		const kind = current.getKindName()
		if (kind === 'ExportSpecifier' || kind === 'ExportDeclaration') {
			return false
		}
		current = current.getParent()
	}

	return true
}

function isFrameworkConsumedExport(
	framework: Parameters<Detector>[1]['framework'],
	filePath: string,
	symbol: string
): boolean {
	// Always skip .next/ build output — these are generated and never imported by user code
	if (/\/\.next\//.test(filePath)) return true

	// Root config files always consume their default export (tools read the file directly)
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
		)
			return true

		if (isRouteHandler && /\/app\/.*\/route\.(ts|tsx)$/.test(filePath)) return true

		// Route segment config exports are consumed by Next.js from page, layout, and route files
		if (isRouteSegmentConfig && isInAppDir && /\/(page|layout|route)\.(ts|tsx)$/.test(filePath))
			return true

		// OG/Twitter image metadata exports
		if (isOgImageMeta && /\/app\/.*\/(opengraph|twitter)-image\.tsx$/.test(filePath))
			return true
	}

	return false
}

function matchesExportedNode(
	symbol: string,
	exportedNode: { getSourceFile(): { getFilePath(): string }; getStart(): number },
	targetNode: { getSourceFile(): { getFilePath(): string }; getStart(): number }
): boolean {
	if (symbol === 'default') {
		return (
			exportedNode.getSourceFile().getFilePath() === targetNode.getSourceFile().getFilePath()
		)
	}

	return (
		exportedNode.getSourceFile().getFilePath() === targetNode.getSourceFile().getFilePath() &&
		exportedNode.getStart() === targetNode.getStart()
	)
}

function hasNamespaceConsumer(
	project: Parameters<Detector>[0]['project'],
	reachable: Parameters<Detector>[0]['reachable'],
	path: string,
	symbol: string,
	node: { getSourceFile(): { getFilePath(): string }; getStart(): number }
): boolean {
	for (const consumerPath of reachable) {
		if (consumerPath === path) continue

		const sourceFile = project.getSourceFile(consumerPath)
		if (!sourceFile) continue

		for (const decl of sourceFile.getImportDeclarations()) {
			const namespaceImport = decl.getNamespaceImport()
			if (!namespaceImport) continue

			const importedFile = decl.getModuleSpecifierSourceFile()
			if (!importedFile) continue

			const exportedNodes = importedFile.getExportedDeclarations().get(symbol)
			if (
				!exportedNodes?.some((exportedNode) =>
					matchesExportedNode(symbol, exportedNode, node)
				)
			) {
				continue
			}

			const alias = namespaceImport.getText()
			const propertyAccesses = sourceFile.getDescendantsOfKind(
				SyntaxKind.PropertyAccessExpression
			)

			if (
				propertyAccesses.some(
					(expr) => expr.getExpression().getText() === alias && expr.getName() === symbol
				)
			) {
				return true
			}
		}
	}

	return false
}

export function detect(
	...[{ project, reachable, analysisBoundaries }, config]: Parameters<Detector>
): ReturnType<Detector> {
	const severity = config.detectors.unusedExports?.severity ?? 'warn'
	const allowed = new Set(config.allowedExports)
	const ignored = new Set(config.ignore.symbols)
	const findings: Finding[] = []
	const hasDynamicImportBoundary = analysisBoundaries.some(
		(boundary) => boundary.kind === 'dynamic-import'
	)

	for (const path of reachable) {
		const sf = project.getSourceFile(path)
		if (!sf) continue

		for (const [symbol, nodes] of sf.getExportedDeclarations()) {
			if (isFrameworkConsumedExport(config.framework, path, symbol)) continue
			if (allowed.has(symbol)) continue
			if (ignored.has(symbol)) continue

			const node = nodes[0]
			if (!node) continue

			if (hasNamespaceConsumer(project, reachable, path, symbol, node)) {
				continue
			}

			const references = 'findReferences' in node ? node.findReferences() : []
			const external: any[] = []
			for (const ref of references) {
				for (const refEntry of ref.getReferences()) {
					const refSourceFile = refEntry.getSourceFile()
					const consumerPath = refSourceFile.getFilePath()
					if (
						consumerPath !== path &&
						reachable.has(consumerPath) &&
						countsAsConsumer(refEntry)
					) {
						external.push(refEntry)
					}
				}
			}

			if (external.length === 0) {
				const { line, column } = node.getSourceFile().getLineAndColumnAtPos(node.getStart())
				const kind = getExportKind(node)

				findings.push({
					detectorId: 'unusedExports',
					severity,
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
	}

	return findings
}
