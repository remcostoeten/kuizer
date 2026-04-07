import { SyntaxKind, type Node, type SourceFile } from 'ts-morph'
import type { FixPlanner } from '../types.js'

const SUPPORTED_UNUSED_EXPORT_DETECTOR_IDS = new Set([
	'unusedExports',
	'unusedFunctions',
	'unusedTypes',
	'unusedClasses',
	'unusedComponents',
	'unusedConstants',
	'unusedEnums',
	'unusedDefaults',
	'unusedOthers',
])

/** Node kinds that represent a complete exported declaration we know how to handle. */
const SUPPORTED_DECL_KINDS = new Set([
	SyntaxKind.FunctionDeclaration,
	SyntaxKind.ClassDeclaration,
	SyntaxKind.InterfaceDeclaration,
	SyntaxKind.TypeAliasDeclaration,
	SyntaxKind.EnumDeclaration,
	SyntaxKind.VariableStatement,
	SyntaxKind.ExportAssignment,
])

/** Get the top-level statement that owns this node (the statement we'd delete or strip export from). */
function getOwningStatement(node: Node, sourceFile: SourceFile): Node | undefined {
	const statements = sourceFile.getStatements()
	for (const stmt of statements) {
		if (stmt === node) return stmt
		if (node.getStart() >= stmt.getStart() && node.getEnd() <= stmt.getEnd()) {
			return stmt
		}
	}
	return undefined
}

/** Check if a symbol is referenced locally within the same file (excluding its own declaration). */
function hasLocalReferences(sourceFile: SourceFile, node: Node): boolean {
	if (!('findReferences' in node)) return false

	const references = (node as any).findReferences()
	for (const ref of references) {
		for (const refEntry of ref.getReferences()) {
			const refPath = refEntry.getSourceFile().getFilePath()
			if (refPath !== sourceFile.getFilePath()) continue

			// Skip the declaration itself
			if (refEntry.isDefinition()) continue

			return true
		}
	}

	return false
}

function getLineRange(node: Node): { startLine: number; endLine: number } {
	const sf = node.getSourceFile()
	const startLine = sf.getLineAndColumnAtPos(node.getStart()).line
	const endLine = sf.getLineAndColumnAtPos(node.getEnd()).line
	return { startLine, endLine }
}

export const planUnusedExportsFix: FixPlanner = (finding, ctx, _config) => {
	if (!SUPPORTED_UNUSED_EXPORT_DETECTOR_IDS.has(finding.detectorId)) {
		return null
	}

	if (finding.symbol === 'default') {
		return {
			supported: false,
			finding,
			reason: 'Default exports are not supported',
		}
	}

	if (finding.context?.hasDynamicImportBoundary === true) {
		return {
			supported: false,
			finding,
			reason: 'Dynamic import analysis boundaries make export deletion uncertain',
		}
	}

	const sourceFile = ctx.project.getSourceFile(finding.file)
	if (!sourceFile) {
		return {
			supported: false,
			finding,
			reason: 'Finding file is not available in the current project',
		}
	}

	const symbol = finding.symbol
	if (!symbol) {
		return {
			supported: false,
			finding,
			reason: 'Finding has no symbol name',
		}
	}

	// ── Resolve the declaration node ─────────────────────────────────

	const exportedDeclarationNodes = sourceFile.getExportedDeclarations().get(symbol)
	const node = exportedDeclarationNodes?.[0]

	if (!node) {
		return {
			supported: false,
			finding,
			reason: 'Could not resolve the exported declaration node',
		}
	}

	// ── Handle re-exports: `export { X } from '...'` or `export { X }` ──

	const owningStmt = getOwningStatement(node, sourceFile)

	// For re-exports (ExportDeclaration with module specifier), we handle them as delete-declaration
	// For default exports via ExportAssignment, we handle them similarly

	const stmtKind = owningStmt?.getKind()
	const isExportDeclaration = stmtKind === SyntaxKind.ExportDeclaration
	const isExportAssignment = stmtKind === SyntaxKind.ExportAssignment
	const isKnownDeclKind = stmtKind !== undefined && SUPPORTED_DECL_KINDS.has(stmtKind)

	if (!isExportDeclaration && !isExportAssignment && !isKnownDeclKind) {
		return {
			supported: false,
			finding,
			reason: `Unsupported declaration kind: ${owningStmt?.getKindName() ?? 'unknown'}`,
		}
	}

	if (isExportDeclaration) {
		return {
			supported: false,
			finding,
			reason: 'Only simple whole-declaration exports are supported',
		}
	}

	if (isExportAssignment) {
		return {
			supported: false,
			finding,
			reason: 'Default exports are not supported',
		}
	}

	// ── Strategy selection ────────────────────────────────────────────

	const stmtNode = owningStmt!
	const statements = sourceFile.getStatements()
	const isOnlyStatement = statements.length === 1
	const locallyReferenced = hasLocalReferences(sourceFile, node)
	const allExportedDeclarations = sourceFile.getExportedDeclarations()
	const exportsExactlySymbol =
		allExportedDeclarations.size === 1 && allExportedDeclarations.has(symbol)

	// Strategy 1: If locally referenced, just strip the export keyword
	if (locallyReferenced && !isExportDeclaration && !isExportAssignment) {
		const { startLine, endLine } = getLineRange(stmtNode)

		return {
			supported: true,
			finding,
			edits: [
				{
					kind: 'remove-export-keyword',
					file: finding.file,
					exportName: symbol,
					startLine,
					endLine,
					description: `Remove 'export' keyword from '${symbol}' (still used locally)`,
				},
			],
			safety: 'safe',
			reason: `Strip export keyword — '${symbol}' is used locally but has no external consumers`,
		}
	}

	// Strategy 3: If it's the only statement AND not locally referenced, delete the whole file
	if (isOnlyStatement && !locallyReferenced && exportsExactlySymbol) {
		if (stmtKind === SyntaxKind.VariableStatement) {
			const variableStatement = stmtNode.asKindOrThrow(SyntaxKind.VariableStatement)
			const declarationCount = variableStatement.getDeclarationList().getDeclarations().length
			if (declarationCount !== 1) {
				return {
					supported: false,
					finding,
					reason: 'Only simple whole-declaration exports are supported',
				}
			}
		}

		return {
			supported: true,
			finding,
			edits: [
				{
					kind: 'delete-file',
					file: finding.file,
					description: `Delete unused export '${symbol}'`,
				},
			],
			safety: 'destructive',
			reason: `File contains only the unused export '${symbol}' — safe to delete entirely`,
		}
	}

	// Strategy 4: Multi-statement file, not locally referenced — delete just the declaration
	if (!locallyReferenced) {
		const { startLine, endLine } = getLineRange(stmtNode)

		// For VariableStatement with multiple declarators, we can't safely
		// delete just one declarator from `export const a = 1, b = 2`
		if (stmtKind === SyntaxKind.VariableStatement) {
			const variableStatement = stmtNode.asKindOrThrow(SyntaxKind.VariableStatement)
			const declarationCount = variableStatement.getDeclarationList().getDeclarations().length
			if (declarationCount !== 1) {
				return {
					supported: false,
					finding,
					reason: 'Only simple whole-declaration exports are supported',
				}
			}
		}

		return {
			supported: true,
			finding,
			edits: [
				{
					kind: 'delete-declaration',
					file: finding.file,
					exportName: symbol,
					startLine,
					endLine,
					description: `Delete unused export '${symbol}'`,
				},
			],
			safety: 'cautious',
			reason: `Delete unused declaration '${symbol}' from multi-statement file`,
		}
	}

	// Fallback: locally referenced default/re-export — not trivially strippable
	return {
		supported: false,
		finding,
		reason: 'Declaration is locally referenced in a way that cannot be trivially de-exported',
	}
}
