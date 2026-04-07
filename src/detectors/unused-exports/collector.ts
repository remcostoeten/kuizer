import type { Detector } from '../types.js'
import type { Finding, ExportKind } from '../../types.js'

type DetectorContext = Parameters<Detector>[0]
type DetectorConfig = Parameters<Detector>[1]

export type ExportedNode = {
	getKindName(): string
	getSourceFile(): {
		getFilePath(): string
		getLineAndColumnAtPos(pos: number): { line: number; column: number }
	}
	getStart(): number
}

export type ExportFinding = Finding & Readonly<{ detectorId: string; symbol: string }>

export type SourceFileWithExports = {
	getFilePath(): string
	getExportedDeclarations(): ReadonlyMap<string, ExportedNode[]>
	getExportSymbols(): {
		getName(): string
		getAliasedSymbol(): { getName(): string } | undefined
	}[]
	getExportDeclarations(): ExportDeclarationLike[]
}

export type ExportDeclarationLike = {
	getModuleSpecifierValue?(): string | undefined
	getNamedExports(): Array<{
		getAliasNode?(): { getText(): string } | undefined
		getName(): string
	}>
}

export function getExportKind(
	sourceFile: SourceFileWithExports,
	symbol: string,
	node: ExportedNode | undefined
): ExportKind {
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

export function isLocallyReexported(
	sourceFile: { getExportDeclarations(): ExportDeclarationLike[] },
	symbol: string
): boolean {
	for (const decl of sourceFile.getExportDeclarations()) {
		if (typeof decl.getModuleSpecifierValue === 'function' && decl.getModuleSpecifierValue()) {
			continue
		}

		for (const specifier of decl.getNamedExports()) {
			const aliasNode =
				typeof specifier.getAliasNode === 'function' ? specifier.getAliasNode() : undefined
			const exportedName = aliasNode ? aliasNode.getText() : specifier.getName()
			if (exportedName === symbol) {
				return true
			}
		}
	}

	return false
}

export function isFrameworkConsumedExport(
	framework: DetectorConfig['framework'],
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

export function createExportKey(symbol: string, node: ExportedNode): string {
	return `${node.getSourceFile().getFilePath()}:${node.getStart()}:${symbol}`
}

export type CollectedExport = {
	symbol: string
	kind: ExportKind
	filePath: string
	line: number
	col: number
	node: ExportedNode
}

export function collectExports(
	ctx: DetectorContext,
	config: DetectorConfig
): CollectedExport[] {
	const { project, reachable } = ctx
	const collected: CollectedExport[] = []

	for (const path of reachable) {
		const sf = project.getSourceFile(path)
		if (!sf) continue

		for (const [symbol, nodes] of sf.getExportedDeclarations()) {
			if (isFrameworkConsumedExport(config.framework, path, symbol)) continue

			const node = nodes[0]
			if (!node) continue

			if (node.getSourceFile().getFilePath() !== path) {
				continue
			}

			const { line, column } = node.getSourceFile().getLineAndColumnAtPos(node.getStart())
			const kind = getExportKind(sf, symbol, node)

			collected.push({
				symbol,
				kind,
				filePath: path,
				line,
				col: column,
				node,
			})
		}
	}

	return collected
}
