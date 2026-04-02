import type { AnalysisBoundary, Finding } from '../types.js'

export type Report = Readonly<{
  findings: readonly Finding[]
  durationMs: number
  analysisBoundaries: readonly AnalysisBoundary[]
}>

/** Pure sink — receives the analysis report after all detectors run. */
export type Reporter = (report: Report) => void
