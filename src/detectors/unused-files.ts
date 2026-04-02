import type { Detector } from './types.js'

export function detect(...[{ all, reachable, analysisBoundaries }, config]: Parameters<Detector>): ReturnType<Detector> {
  const severity = config.detectors.unusedFiles?.severity ?? 'warn'
  const ignored  = new Set(config.ignore.files)
  const hasDynamicImportBoundary = analysisBoundaries.some(
    (boundary) => boundary.kind === 'dynamic-import',
  )

  return [...all]
    .filter(f => !reachable.has(f))
    .filter(f => !ignored.has(f))
    .map(file => ({
      detectorId: 'unusedFiles',
      severity,
      file,
      line:    1,
      col:     1,
      message: hasDynamicImportBoundary
        ? 'File is unreachable from any statically resolved entry point'
        : 'File is unreachable from any entry point',
      fixable: false,
      ...(hasDynamicImportBoundary
        ? {
            context: {
              analysisBoundaryCount: analysisBoundaries.length,
              hasDynamicImportBoundary: true,
            },
          }
        : {}),
    }))
}
