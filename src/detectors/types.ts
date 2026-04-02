import type { Ctx, Config, DetectorResult } from '../types.js'

/** Pure function — takes analysis context and resolved config, returns findings. */
export type Detector = (ctx: Ctx, config: Config) => DetectorResult
