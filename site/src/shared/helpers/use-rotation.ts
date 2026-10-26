import { useEffect, useState } from 'react'

/**
 * @name useRotation
 * @description Steps an index through 0..length-1 on a fixed interval and
 * wraps around. It stays on 0 when paused or when there is nothing to
 * rotate to.
 *
 * @example
 * const index = useRotation(commands.length, 5000, hovered)
 */
export function useRotation(length: number, interval: number, paused = false): number {
  const [index, setIndex] = useState(0)

  useEffect(() => {
    if (paused || length < 2) return
    const timer = window.setInterval(() => setIndex((current) => (current + 1) % length), interval)
    return () => window.clearInterval(timer)
  }, [length, interval, paused])

  return index
}
