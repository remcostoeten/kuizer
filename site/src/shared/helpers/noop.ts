/**
 * @name noop
 * @description Does nothing. Call it inside a catch block that swallows an
 * error on purpose, so the intent is visible.
 *
 * @example
 * try {
 *   localStorage.setItem('theme', 'light')
 * } catch {
 *   noop()
 * }
 */
export function noop() {}
