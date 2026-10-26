'use client'

import { useEffect, useRef, useState } from 'react'

type SwapProps = {
  items: readonly string[]
  index: number
  roll?: boolean
  className?: string
}

type Poses = {
  active: string
  leaving: string
  waiting: string
}

const fadePoses: Poses = {
  active: 'opacity-100 blur-[0px] translate-y-0',
  leaving: 'opacity-0 blur-[6px] -translate-y-[0.7em]',
  waiting: 'opacity-0 blur-[6px] translate-y-[0.7em]',
}

const rollPoses: Poses = {
  active: 'translate-y-0',
  leaving: '-translate-y-[140%]',
  waiting: 'translate-y-[140%]',
}

function poseOf(poses: Poses, position: number, index: number, count: number) {
  if (position === index) return poses.active
  if (position === (index - 1 + count) % count) return poses.leaving
  return poses.waiting
}

export function Swap({ items, index, roll = false, className = '' }: SwapProps) {
  const poses = roll ? rollPoses : fadePoses
  const refs = useRef<Array<HTMLSpanElement | null>>([])
  const [widths, setWidths] = useState<number[] | null>(null)

  useEffect(() => {
    const observer = new ResizeObserver(() => {
      setWidths(refs.current.map((node) => node?.getBoundingClientRect().width ?? 0))
    })
    for (const node of refs.current) if (node) observer.observe(node)
    return () => observer.disconnect()
  }, [])

  const [step, setStep] = useState({ index, growing: false })
  if (step.index !== index) {
    setStep({ index, growing: widths !== null && (widths[index] ?? 0) > (widths[step.index] ?? 0) })
  }
  const widthMotion = step.growing ? 'duration-[200ms] ease-out' : 'duration-[480ms] ease-swap'

  return (
    <span
      className={`relative inline-block whitespace-nowrap transition-[width] ${widthMotion} ${roll ? 'roll-window' : ''} ${className}`}
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
          className={`absolute top-0 left-1/2 -translate-x-1/2 swap-item ${poseOf(poses, position, index, items.length)}`}
        >
          {item}
        </span>
      ))}
    </span>
  )
}
