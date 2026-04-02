import type { Ctx, Config, Finding } from '../types.js'

/** Pure function — takes analysis context and resolved config, returns findings. */
export type Detector = (ctx: Ctx, config: Config) => Finding[]