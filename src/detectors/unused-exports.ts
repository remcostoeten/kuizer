import { SyntaxKind, Node, ts } from 'ts-morph'
import { dirname, resolve } from 'path'
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
		if (decl.hasModuleSpecifier()) {
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
			/\/(next|drizzle|vitest|vite|playwright|postcss|tailwind|jest)\.config\.[cm]?[jt]sx?$/
		if (rootConfigPattern.test(filePath)) return true
	}

	const appDirPattern = framework === 'nextjs-app'
	const pagesDirPattern = framework === 'nextjs-pages'

	if (!appDirPattern && !pagesDirPattern) return false

	const isDefault = symbol === 'default'
	const isMetadata = symbol === 'metadata'
	const isViewport = symbol === 'viewport'
	const isGenerateStaticParams = symbol === 'generateStaticParams'
	const isGenerateMetadata = symbol === 'generateMetadata'
	const isGenerateViewport = symbol === 'generateViewport'
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

		if ((isMetadata || isViewport) && isInAppDir && /\/(page|layout)\.(tsx|ts)$/.test(filePath)) return true

		if (
			(isGenerateStaticParams || isGenerateMetadata || isGenerateViewport) &&
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

function resolveDynamicImport(
	fromFile: string,
	specifier: string,
	all: ReadonlySet<string>
): string | undefined {
	if (!specifier.startsWith('.')) {
		return undefined
	}

	const base = resolve(dirname(fromFile), specifier)
	const withoutJsExt = base.replace(/\.(mjs|cjs|js|jsx)$/, '')
	const candidates = [
		base,
		withoutJsExt,
		`${base}.ts`,
		`${base}.tsx`,
		`${withoutJsExt}.ts`,
		`${withoutJsExt}.tsx`,
		resolve(base, 'index.ts'),
		resolve(base, 'index.tsx'),
		resolve(withoutJsExt, 'index.ts'),
		resolve(withoutJsExt, 'index.tsx'),
	]

	return candidates.find((candidate) => all.has(candidate))
}

function buildConsumerCounts(
	ctx: Parameters<Detector>[0]
): { prodCounts: Map<string, number>, testCounts: Map<string, number> } {
	const { project, reachable, testReachable, all, cache } = ctx
	const prodCounts = new Map<string, number>()
	const testCounts = new Map<string, number>()

	const processConsumers = (files: ReadonlySet<string>, counts: Map<string, number>) => {
		for (const consumerPath of files) {
			const cachedKeys = cache.getCachedFileContext(consumerPath)?.consumerKeys
			if (cachedKeys) {
				for (const key of cachedKeys) {
					counts.set(key, (counts.get(key) ?? 0) + 1)
				}
				continue
			}

			const discoveredKeys: string[] = []
			const recordConsumer = (sourceFile: SourceFileWithExports, symbol: string) => {
				const key = resolveConsumerKey(sourceFile, symbol)
				if (!key) return
				discoveredKeys.push(key)
				counts.set(key, (counts.get(key) ?? 0) + 1)
			}
			const sourceFile = project.getSourceFile(consumerPath)
			if (!sourceFile) continue

			const namespaceImports = new Map<string, SourceFileWithExports>()

			for (const decl of sourceFile.getImportDeclarations()) {
				const importedFile = decl.getModuleSpecifierSourceFile()
				if (!importedFile) continue

				if (decl.getImportClause()?.isTypeOnly()) {
					const defaultImport = decl.getDefaultImport()
					if (defaultImport) recordConsumer(importedFile, 'default')

					for (const specifier of decl.getNamedImports()) {
						recordConsumer(importedFile, specifier.getName())
					}
					continue
				}

				const defaultImport = decl.getDefaultImport()
				if (defaultImport) recordConsumer(importedFile, 'default')

				for (const specifier of decl.getNamedImports()) {
					recordConsumer(importedFile, specifier.getName())
				}

				const namespaceImport = decl.getNamespaceImport()
				if (namespaceImport) {
					namespaceImports.set(namespaceImport.getText(), importedFile)
				}
			}

			const walkAST = (node: ts.Node) => {
				if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) {
					const specifier = node.arguments[0]
					if (specifier && (ts.isStringLiteral(specifier) || ts.isNoSubstitutionTemplateLiteral(specifier))) {
						const resolvedStr = resolveDynamicImport(sourceFile.getFilePath(), specifier.text, all)
						if (resolvedStr) {
							const resolved = project.getSourceFile(resolvedStr)
							if (resolved) {
								const parent = node.parent
								if (parent && ts.isPropertyAccessExpression(parent) && parent.name.text === 'then') {
									const thenCall = parent.parent
									if (thenCall && ts.isCallExpression(thenCall)) {
										const arrowFn = thenCall.arguments[0]
										if (arrowFn && (ts.isArrowFunction(arrowFn) || ts.isFunctionExpression(arrowFn))) {
											const modParam = arrowFn.parameters[0]
											if (modParam && ts.isIdentifier(modParam.name)) {
												const modName = modParam.name.text
												const searchModAccess = (n: ts.Node) => {
													if (ts.isPropertyAccessExpression(n)) {
														if (ts.isIdentifier(n.expression) && n.expression.text === modName) {
															recordConsumer(resolved, n.name.text)
														}
													}
													ts.forEachChild(n, searchModAccess)
												}
												searchModAccess(arrowFn.body)
											}
										}
									}
								}
							}
						}
					}
				}

				if (namespaceImports.size > 0 && ts.isPropertyAccessExpression(node)) {
					if (ts.isIdentifier(node.expression)) {
						const importedFile = namespaceImports.get(node.expression.text)
						if (importedFile) {
							recordConsumer(importedFile, node.name.text)
						}
					}
				}

				ts.forEachChild(node, walkAST)
			}
			ts.forEachChild(sourceFile.compilerNode, walkAST)

			cache.setCachedFileContext(consumerPath, { consumerKeys: discoveredKeys })
		}
	}

	processConsumers(reachable, prodCounts)
	processConsumers(testReachable, testCounts)

	return { prodCounts, testCounts }
}

function hasInternalConsumer(sourceFile: any, node: any): boolean {
	if (!Node.isNode(node)) return false;

	const nameNode = typeof (node as any).getNameNode === 'function' ? (node as any).getNameNode() : null;
	const name = nameNode ? nameNode.getText() : (typeof (node as any).getName === 'function' ? (node as any).getName() : null);
	if (!name) return false;

	let hasUsage = false;
	const walk = (n: ts.Node) => {
		if (hasUsage) return;
		if (ts.isIdentifier(n) && n.text === name) {
			const isDeclName = nameNode && n === nameNode.compilerNode;
			if (!isDeclName) {
				const parent = n.parent;
				if (!(parent && ts.isExportSpecifier(parent)) && !(parent && ts.isPropertyAccessExpression(parent) && parent.name === n)) {
					// ensure it's not strictly within the node itself
					let current: ts.Node | undefined = n;
					let isInside = false;
					while (current) {
						if (current === node.compilerNode) {
							isInside = true;
							break;
						}
						current = current.parent;
					}
					if (!isInside) {
						hasUsage = true;
					}
				}
			}
		}
		ts.forEachChild(n, walk);
	};
	ts.forEachChild(sourceFile.compilerNode, walk);
	return hasUsage;
}

function collectUnusedExportFindings(
	ctx: Parameters<Detector>[0],
	config: Parameters<Detector>[1]
): ExportFinding[] {
	const { project, reachable, testReachable, all, analysisBoundaries } = ctx
	const allowed = new Set(config.allowedExports)
	const ignored = new Set(config.ignore.symbols)
	const findings: ExportFinding[] = []
	const hasDynamicImportBoundary = analysisBoundaries.some(
		(boundary) => boundary.kind === 'dynamic-import'
	)
	const { prodCounts, testCounts } = buildConsumerCounts(ctx)
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

			const pCount = prodCounts.get(exportKey) ?? 0;
			const tCount = testCounts.get(exportKey) ?? 0;

			if (pCount > 0) {
				continue
			}

			const isTestOnly = tCount > 0 && !config.countTestsAsConsumers;
			let isInternalOnly = false;
			if (!isTestOnly) {
				isInternalOnly = hasInternalConsumer(sf, node);
			}

			const { line, column } = node.getSourceFile().getLineAndColumnAtPos(node.getStart())
			const kind = getExportKind(sf, symbol, node)
			const detectorId = mapDetectorId({ file: path, symbol, kind })

			let message = `Exported symbol '${symbol}' has 0 consumers`;
			if (isTestOnly) {
				message = `Exported symbol '${symbol}' is only used in tests (test-only)`;
			} else if (isInternalOnly) {
				message = `Exported symbol '${symbol}' is only used internally in the same module`;
			} else if (hasDynamicImportBoundary) {
				message = `Exported symbol '${symbol}' has 0 statically resolved consumers`;
			}

			findings.push({
				detectorId,
				severity: getSeverityForDetector(config, detectorId),
				file: path,
				line,
				col: column,
				message,
				symbol,
				kind,
				fixable: false,
				context: {
					consumerCount: 0,
					framework: config.framework,
					analysisBoundaryCount: analysisBoundaries.length,
					hasDynamicImportBoundary,
					testOnly: isTestOnly,
					internalOnly: isInternalOnly
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
