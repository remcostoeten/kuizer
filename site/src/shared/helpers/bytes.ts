export const KiB = 1024
export const MiB = 1024 ** 2
export const GiB = 1024 ** 3

const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'] as const

/**
 * @name formatBytes
 * @description Formats a byte count the way the kuizer binary does: whole
 * bytes below 1 KiB, otherwise one decimal with a binary unit.
 *
 * @example
 * formatBytes(14.2 * GiB) // '14.2 GiB'
 */
export function formatBytes(bytes: number): string {
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  if (unit === 0) return `${Math.round(bytes)} B`
  return `${value.toFixed(1)} ${units[unit]}`
}

/**
 * @name formatCount
 * @description Formats an integer with thousands separators for the scan
 * progress and entry counts.
 *
 * @example
 * formatCount(184230) // '184,230'
 */
export function formatCount(count: number): string {
  return new Intl.NumberFormat('en-US').format(Math.round(count))
}
