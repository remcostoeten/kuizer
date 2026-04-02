import type { Finding } from '../types.js'
import { getVersion } from '../meta.js'

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
export function json(
  findings: readonly Finding[],
  durationMs: number,
): void {
  process.stdout.write(
    JSON.stringify(
      {
        meta: {
          version,
          durationMs,
          total: findings.length,
        },
        findings,
      },
      null,
      2,
    ) + '\n',
  )
}
