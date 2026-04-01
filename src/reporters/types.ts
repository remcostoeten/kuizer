import type { Finding } from '../types.js'

/** Pure sink — receives all findings after all detectors have run. */
export type Reporter = (findings: readonly Finding[], durationMs: number) => void