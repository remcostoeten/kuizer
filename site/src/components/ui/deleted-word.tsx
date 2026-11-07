'use client'

import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { noop } from '@/shared/helpers/noop'

type Target = {
  word: string
  size: number
  unit: 'GB' | 'MB'
}

type Phase = 'enter' | 'idle' | 'mark' | 'fall'

type DeletedWordProps = {
  items: readonly Target[]
  interval: number
  className?: string
}

const markBefore = 1500
const fallBefore = 1250
const countFor = 650
const tilts = ['-7deg', '4deg', '7deg']
const reducedMotion = '(prefers-reduced-motion: reduce)'

function subscribeMotion(onChange: () => void) {
  const query = window.matchMedia(reducedMotion)
  query.addEventListener('change', onChange)
  return () => query.removeEventListener('change', onChange)
}

function readMotion(): 'on' | 'off' {
  return window.matchMedia(reducedMotion).matches ? 'off' : 'on'
}

function readServerMotion(): 'on' | 'off' {
  return 'on'
}

function formatSize(value: number, unit: Target['unit']): string {
  return `${value.toFixed(unit === 'GB' ? 1 : 0)} ${unit}`
}

/**
 * @name DeletedWord
 * @description Cycles through a list of reclaimable targets the way a row
 * goes through kuizer: the word is marked with a brand sweep, its letters
 * fall away one by one while the reclaimed size counts up in their place,
 * then the next word rises in. With reduced motion every step is a cut.
 *
 * @example
 * <DeletedWord items={[{ word: 'node_modules', size: 1.4, unit: 'GB' }]} interval={4000} />
 */
export function DeletedWord({ items, interval, className = '' }: DeletedWordProps) {
  const [index, setIndex] = useState(0)
  const [phase, setPhase] = useState<Phase>('idle')
  const [counted, setCounted] = useState<Target | undefined>(items[0])
  const motion = useSyncExternalStore(subscribeMotion, readMotion, readServerMotion)
  const reduced = motion === 'off'
  const refs = useRef<Array<HTMLSpanElement | null>>([])
  const [widths, setWidths] = useState<number[] | null>(null)
  const current = items[index] ?? items[0]

  useEffect(() => {
    const observer = new ResizeObserver(() => {
      setWidths(refs.current.map((node) => node?.getBoundingClientRect().width ?? 0))
    })
    for (const node of refs.current) if (node) observer.observe(node)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    if (items.length < 2) return noop
    const timers: number[] = []
    function cycle() {
      timers.length = 0
      timers.push(window.setTimeout(() => setPhase('mark'), interval - markBefore))
      timers.push(window.setTimeout(() => setPhase('fall'), interval - fallBefore))
      timers.push(
        window.setTimeout(() => {
          setIndex((at) => (at + 1) % items.length)
          setPhase(reduced ? 'idle' : 'enter')
          cycle()
        }, interval),
      )
    }
    cycle()
    return () => {
      for (const timer of timers) window.clearTimeout(timer)
    }
  }, [items.length, interval, reduced])

  useEffect(() => {
    if (phase !== 'fall' || reduced || !current) return noop
    const start = performance.now()
    const target = current
    let frame = 0
    function tick(now: number) {
      const progress = Math.min((now - start) / countFor, 1)
      setCounted({ ...target, size: target.size * (1 - (1 - progress) ** 3) })
      if (progress < 1) frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [phase, reduced, current])

  if (!current) return null
  const shown = reduced || !counted ? current : counted

  return (
    <span
      data-phase={phase}
      data-motion={motion}
      className={`relative inline-block whitespace-nowrap transition-[width] duration-[320ms] ease-out ${className}`}
      style={widths ? { width: widths[index] } : undefined}
    >
      <span aria-hidden="true" className="invisible">
        {widths ? '​' : current.word}
      </span>
      {items.map((item, position) => (
        <span
          key={item.word}
          ref={(node) => {
            refs.current[position] = node
          }}
          aria-hidden="true"
          className="invisible absolute top-0 left-0"
        >
          {item.word}
        </span>
      ))}
      <span key={index} className="word">
        <span className="sr-only">{current.word}</span>
        <span aria-hidden="true">
          {Array.from(current.word, (char, position) => (
            <span
              key={position}
              className="char"
              style={{ '--i': position, '--tilt': tilts[position % tilts.length] } as React.CSSProperties}
            >
              {char}
            </span>
          ))}
        </span>
        <i className="sweep" aria-hidden="true" />
      </span>
      <i className="tag not-italic" aria-hidden="true">
        −{formatSize(shown.size, shown.unit)}
      </i>
    </span>
  )
}
