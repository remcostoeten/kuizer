export function clamp(n: number, min: number, max: number): number {
  return Math.min(Math.max(n, min), max)
}

export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t
}
