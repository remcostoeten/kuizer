/**
 * @name plural
 * @description Joins a count and a noun, adding an s unless the count is 1.
 *
 * @example
 * plural(3, 'item') // '3 items'
 */
export function plural(count: number, word: string): string {
  return `${count} ${word}${count === 1 ? '' : 's'}`
}

/**
 * @name uncommitted
 * @description Phrases a project's uncommitted change count the way the
 * kuizer binary does in its warnings.
 *
 * @example
 * uncommitted(1) // '1 uncommitted change'
 */
export function uncommitted(changes: number): string {
  return plural(changes, 'uncommitted change')
}
