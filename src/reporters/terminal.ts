import { write, badge, bold, dim, rule, indent } from '../output/index.js'
import type { Finding  } from '../types.js'

function groupByFile(findings: readonly Finding[]): Map<string, Finding[]> {
  return findings.reduce((acc, f) => {
    const group = acc.get(f.file) ?? []
    group.push(f)
    return acc.set(f.file, group)
  }, new Map<string, Finding[]>())
}

function summary(findings: readonly Finding[], durationMs: number): string {
  const errors   = findings.filter(f => f.severity === 'error').length
  const warnings = findings.filter(f => f.severity === 'warn').length
  const s        = `${(durationMs / 1000).toFixed(1)}s`
  return dim(`${errors} errors · ${warnings} warnings · ${s}`)
}

  
export function report(findings: readonly Finding[], durationMs: number) {
if (findings.length === 0) {
    write(`\n${dim('No findings.')}`)
    write(rule())
    return
  }

  for (const [file, group] of groupByFile(findings)) {
    write(`\n${bold(file)}`)
    for (const f of group) {
      const loc  = dim(`L${f.line}`)
      const id   = dim(f.detectorId)
      write(indent(`${badge(f.severity)}  ${loc}  ${id}  ${f.message}`))
    }
  }

  write(`\n${rule()}`)
  write(summary(findings, durationMs))
}