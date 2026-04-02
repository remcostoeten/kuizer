export function formatDate(d: Date): string {
  return d.toISOString().split('T')[0] ?? ''
}

export function formatBytes(n: number): string {
  return `${(n / 1024).toFixed(2)} KB`
}

export function formatPercent(n: number): string {
  return `${(n * 100).toFixed(1)}%`
}
