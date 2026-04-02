import { greet } from './lib/greet.js'
import { Logger } from './lib/logger.js'
import type { ID, Result } from './types/index.js'

const logger = new Logger('app')

logger.info(greet('world'))

function resolveId(id: ID): Result<string> {
  return { ok: true, value: id }
}

resolveId('bootstrap')
