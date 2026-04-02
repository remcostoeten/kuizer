import type { Detector } from './types.js'
import type { Finding }  from '../types.js'

export function detect(...[{ project, reachable }, config]: Parameters<Detector>): ReturnType<Detector> {
  const severity = config.detectors.unusedExports?.severity ?? 'warn'
  const allowed  = new Set(config.allowedExports)
  const ignored  = new Set(config.ignore.symbols)
  const findings: Finding[] = []

  for (const path of reachable) {
    const sf = project.getSourceFile(path)
    if (!sf) continue

    for (const [symbol, nodes] of sf.getExportedDeclarations()) {
      if (allowed.has(symbol)) continue
      if (ignored.has(symbol))  continue

      const node = nodes[0]
      if (!node) continue


      const references = 'findReferences' in node ? node.findReferences() : []
      const external: any[] = [];
      for (const ref of references) {
        for (const refEntry of ref.getReferences()) {
          const refSourceFile = refEntry.getSourceFile();
          if (refSourceFile.getFilePath() !== path) {
            external.push(refEntry);
          }
        }
      }

      if (external.length === 0) {
        findings.push({
          detectorId: 'unusedExports',
          severity,
          file:    path,
          line:    node.getStartLineNumber(),
          col:     node.getStartLinePos(),
          message: `Exported symbol '${symbol}' has 0 consumers`,
          symbol,
          fixable: false,
        })
      }
    }
  }

  return findings
}