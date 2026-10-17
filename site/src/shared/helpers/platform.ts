import { noop } from '@/shared/helpers/noop'

export type Platform = 'Linux' | 'macOS' | null

/**
 * @name readPlatform
 * @description Guesses the desktop platform from the user agent so the
 * download button can name it. Returns null for anything that is not a
 * Linux or macOS desktop.
 *
 * @example
 * const platform = useSyncExternalStore(subscribePlatform, readPlatform, readServerPlatform)
 */
export function readPlatform(): Platform {
  const agent = navigator.userAgent
  if (/Mac/.test(agent) && !/iPhone|iPad/.test(agent)) return 'macOS'
  if (/Linux/.test(agent) && !/Android/.test(agent)) return 'Linux'
  return null
}

/**
 * @name readServerPlatform
 * @description The platform value used during server rendering, where no
 * user agent is read.
 *
 * @example
 * useSyncExternalStore(subscribePlatform, readPlatform, readServerPlatform)
 */
export function readServerPlatform(): Platform {
  return null
}

/**
 * @name subscribePlatform
 * @description The user agent never changes during a page view, so there is
 * nothing to subscribe to. Exists to satisfy useSyncExternalStore.
 *
 * @example
 * useSyncExternalStore(subscribePlatform, readPlatform, readServerPlatform)
 */
export function subscribePlatform() {
  return noop
}
