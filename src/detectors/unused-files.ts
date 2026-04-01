import type { Detector } from './types.js'

export function detect(...[{ all, reachable }, config]: Parameters<Detector>): ReturnType<Detector> {
  const severity = config.detectors.unusedFiles?.severity ?? 'warn'
  const ignored  = new Set(config.ignore.files)

  return [...all]
    .filter(f => !reachable.has(f))
    .filter(f => !ignored.has(f))
    .map(file => ({
      detectorId: 'unusedFiles',
      severity,
      file,
      line:    1,
      col:     1,
      message: 'File is unreachable from any entry point',
      fixable: false,
    }))
}