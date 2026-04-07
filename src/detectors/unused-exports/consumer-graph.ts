import { SyntaxKind } from 'ts-morph'
import type { Project } from 'ts-morph'

type ExportedNode = {
	getStart(): number
	getSourceFile(): {
		getFilePath(): string
	}
}

type SourceFileWithExports = {
	getFilePath(): string
	getExportedDeclarations(): ReadonlyMap<string, ExportedNode[]>
	getExportSymbols(): {
		getName(): string
		getAliasedSymbol(): { getName(): string } | undefined
	}[]
}

export type ReferenceEntryLike = {
	getSourceFile(): {
		getFilePath(): string
	}
	isDefinition(): boolean | undefined
}

export type ReferenceSymbolLike = {
	getReferences(): ReferenceEntryLike[]
}

export type ReferenceableNode = {
	findReferences(): ReferenceSymbolLike[]
}

function createExportKey(symbol: string, node: ExportedNode): string {
	return `${node.getSourceFile().getFilePath()}:${node.getStart()}:${symbol}`
}

export function hasLocalReferences(node: ReferenceableNode | undefined, filePath: string): boolean {
	if (!node) {
		return false
	}

	for (const reference of node.findReferences()) {
		for (const entry of reference.getReferences()) {
			if (entry.getSourceFile().getFilePath() !== filePath) continue
			if (entry.isDefinition()) continue
			return true
		}
	}

	return false
}

export function resolveConsumerKey(sourceFile: SourceFileWithExports, symbol: string): string | undefined {
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

export function buildConsumerCounts(
	project: Project,
	reachable: ReadonlySet<string>
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

			const defaultImport = decl.getDefaultImport()
			if (defaultImport && hasLocalReferences(defaultImport, consumerPath)) {
				recordConsumer(importedFile, 'default')
			}

			for (const specifier of decl.getNamedImports()) {
				const localNode = specifier.getAliasNode() ?? specifier.getNameNode()
				if (hasLocalReferences(localNode, consumerPath)) {
					recordConsumer(importedFile, specifier.getName())
				}
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
