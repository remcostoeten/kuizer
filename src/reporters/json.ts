import type { Reporter } from './types.js'

/** 
 * @name JSON Reporter
 * @description A simple reporter that outputs findings as JSON to stdout. Useful for CI or when you want to process the output with other tools.
 *
 * @example
 * {
 *   "meta": { "version": "0.0.1", "durationMs": 123, "total": 5 },
 *   "findings": [ ... ]
 * }
 */


export const report: Reporter = (findings, durationMs) => {
  process.stdout.write(JSON.stringify({
    meta: { version: '0.0.1', durationMs, total: findings.length },
    findings,
  }, null, 2) + '\n')
}