import { SyntaxKind } from 'ts-morph'
import type { Detector } from './types.js'
import type { Finding }  from '../types.js'

function countsAsConsumer(refEntry: { getNode(): { getKindName(): string; getParent(): any } }): boolean {
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
  symbol: string,
): boolean {
  if (symbol !== 'default') {
    return false
  }

  if (framework === 'nextjs-app') {
    return /\/app\/.*\/(page|layout|loading|error|not-found|template|default)\.tsx$/.test(filePath)
  }

  if (framework === 'nextjs-pages') {
    return /\/pages\/(?!api\/).+\.(ts|tsx)$/.test(filePath)
  }

  return false
}

function matchesExportedNode(
  symbol: string,
  exportedNode: { getSourceFile(): { getFilePath(): string }; getStart(): number },
  targetNode: { getSourceFile(): { getFilePath(): string }; getStart(): number },
): boolean {
  if (symbol === 'default') {
    return exportedNode.getSourceFile().getFilePath() === targetNode.getSourceFile().getFilePath()
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
  node: { getSourceFile(): { getFilePath(): string }; getStart(): number },
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
      if (!exportedNodes?.some(exportedNode => matchesExportedNode(symbol, exportedNode, node))) {
        continue
      }

      const alias = namespaceImport.getText()
      const propertyAccesses = sourceFile.getDescendantsOfKind(SyntaxKind.PropertyAccessExpression)

      if (propertyAccesses.some((expr) =>
        expr.getExpression().getText() === alias &&
        expr.getName() === symbol,
      )) {
        return true
      }
    }
  }

  return false
}

export function detect(...[{ project, reachable, analysisBoundaries }, config]: Parameters<Detector>): ReturnType<Detector> {
  const severity = config.detectors.unusedExports?.severity ?? 'warn'
  const allowed  = new Set(config.allowedExports)
  const ignored  = new Set(config.ignore.symbols)
  const findings: Finding[] = []
  const hasDynamicImportBoundary = analysisBoundaries.some(
    (boundary) => boundary.kind === 'dynamic-import',
  )

  for (const path of reachable) {
    const sf = project.getSourceFile(path)
    if (!sf) continue

    for (const [symbol, nodes] of sf.getExportedDeclarations()) {
      if (isFrameworkConsumedExport(config.framework, path, symbol)) continue
      if (allowed.has(symbol)) continue
      if (ignored.has(symbol))  continue

      const node = nodes[0]
      if (!node) continue

      if (hasNamespaceConsumer(project, reachable, path, symbol, node)) {
        continue
      }

      const references = 'findReferences' in node ? node.findReferences() : []
      const external: any[] = [];
      for (const ref of references) {
        for (const refEntry of ref.getReferences()) {
          const refSourceFile = refEntry.getSourceFile();
          const consumerPath = refSourceFile.getFilePath()
          if (
            consumerPath !== path &&
            reachable.has(consumerPath) &&
            countsAsConsumer(refEntry)
          ) {
            external.push(refEntry);
          }
        }
      }

      if (external.length === 0) {
        const { line, column } = node.getSourceFile().getLineAndColumnAtPos(node.getStart())

        findings.push({
          detectorId: 'unusedExports',
          severity,
          file:    path,
          line,
          col:     column,
          message: hasDynamicImportBoundary
            ? `Exported symbol '${symbol}' has 0 statically resolved consumers`
            : `Exported symbol '${symbol}' has 0 consumers`,
          symbol,
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
