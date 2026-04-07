import { greet } from './lib/greet.js'
import { Logger } from './lib/logger.js'

const logger = new Logger('app')
logger.info(greet('world'))

import type { ID, Result } from './types/index.js'

function resolveId(id: ID): Result<string> {
  return { ok: true, value: id }
}

resolveId('bootstrap')
