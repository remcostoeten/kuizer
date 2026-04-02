import { getVersion } from '../meta.js'
import type { Report } from './types.js'

const version = getVersion(import.meta.url)

/**
 * Outputs findings as formatted JSON to stdout.
 *
 * @remarks
 * Intended for CI pipelines or piping into other tools that consume structured output.
 *
 * @param findings - The list of findings to report.
 * @param durationMs - Execution duration in milliseconds.
 *
 * @example
 * {
 *   "meta": { "version": "0.1.1", "durationMs": 123, "total": 5 },
 *   "findings": [ ... ]
 * }
 */
export function json({
  findings,
  durationMs,
  analysisBoundaries,
}: Report): void {
  process.stdout.write(
    JSON.stringify(
      {
        meta: {
          version,
          durationMs,
          total: findings.length,
          analysisBoundaryCount: analysisBoundaries.length,
        },
        findings,
        analysisBoundaries,
      },
      null,
      2,
    ) + '\n',
  )
}
