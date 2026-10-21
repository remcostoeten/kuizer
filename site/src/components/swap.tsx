'use client'

import { useEffect, useRef, useState } from 'react'
import { useRotation } from '@/shared/helpers/use-rotation'

type SwapProps = {
  items: readonly string[]
  index: number
  className?: string
}

type RotatingWordProps = {
  items: readonly string[]
  interval: number
}

const poses = {
  active: 'opacity-100 blur-[0px] translate-y-0',
  leaving: 'opacity-0 blur-[6px] -translate-y-[0.35em]',
  waiting: 'opacity-0 blur-[6px] translate-y-[0.35em]',
} as const

function poseOf(position: number, index: number, count: number) {
  if (position === index) return poses.active
  if (position === (index - 1 + count) % count) return poses.leaving
  return poses.waiting
}

export function Swap({ items, index, className = '' }: SwapProps) {
  const refs = useRef<Array<HTMLSpanElement | null>>([])
  const [widths, setWidths] = useState<number[] | null>(null)

  useEffect(() => {
    const observer = new ResizeObserver(() => {
      setWidths(refs.current.map((node) => node?.getBoundingClientRect().width ?? 0))
    })
    for (const node of refs.current) if (node) observer.observe(node)
    return () => observer.disconnect()
  }, [])

  return (
    <span
      className={`relative inline-block whitespace-nowrap transition-[width] duration-[480ms] ease-swap motion-reduce:transition-none ${className}`}
      style={widths ? { width: widths[index] } : undefined}
    >
      <span aria-hidden="true" className="invisible">
        {widths ? '​' : items[0]}
      </span>
      {items.map((item, position) => (
        <span
          key={item}
          ref={(node) => {
            refs.current[position] = node
          }}
          aria-hidden={position !== index}
          className={`absolute top-0 left-1/2 -translate-x-1/2 swap-item ${poseOf(position, index, items.length)}`}
        >
          {item}
        </span>
      ))}
    </span>
  )
}

export function RotatingWord({ items, interval }: RotatingWordProps) {
  const index = useRotation(items.length, interval)
  return <Swap items={items} index={index} />
}
