import type { Detector } from './types.js'
import type { Finding } from '../types.js'

function hasLocalReferences(node: any, filePath: string): boolean {
	if (!node || typeof node.findReferences !== 'function') {
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

function createFinding(
	file: string,
	node: {
		getSourceFile(): {
			getLineAndColumnAtPos(pos: number): { line: number; column: number }
		}
		getStart(): number
	},
	symbol: string,
	severity: Finding['severity']
): Finding {
	const { line, column } = node.getSourceFile().getLineAndColumnAtPos(node.getStart())

	return {
		detectorId: 'unusedImports',
		severity,
		file,
		line,
		col: column,
		message: `Imported symbol '${symbol}' has 0 local uses`,
		symbol,
		fixable: false,
		context: {
			consumerCount: 0,
		},
	}
}

export const detect: Detector = ({ project, reachable }, config) => {
	const findings: Finding[] = []
	const severity = config.detectors.unusedImports?.severity ?? 'warn'

	for (const path of reachable) {
		const sourceFile = project.getSourceFile(path)
		if (!sourceFile) continue

		for (const decl of sourceFile.getImportDeclarations()) {
			const defaultImport = decl.getDefaultImport()
			if (defaultImport && !hasLocalReferences(defaultImport, path)) {
				findings.push(createFinding(path, defaultImport, defaultImport.getText(), severity))
			}

			const namespaceImport = decl.getNamespaceImport()
			if (namespaceImport && !hasLocalReferences(namespaceImport, path)) {
				findings.push(
					createFinding(path, namespaceImport, namespaceImport.getText(), severity)
				)
			}

			for (const specifier of decl.getNamedImports()) {
				const localNode = specifier.getAliasNode() ?? specifier.getNameNode()
				const localName = specifier.getAliasNode()?.getText() ?? specifier.getName()
				if (!hasLocalReferences(localNode, path)) {
					findings.push(createFinding(path, localNode, localName, severity))
				}
			}
		}
	}

	return findings
}
