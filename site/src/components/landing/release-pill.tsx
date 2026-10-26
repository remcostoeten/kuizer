'use client'

import { useState } from 'react'
import { RollingNumber } from '@/components/ui/rolling-number'
import { Swap } from '@/components/ui/swap'
import { registries, version } from '@/content/site'
import { useRotation } from '@/shared/helpers/use-rotation'

const names = registries.map((registry) => registry.name)

function PillArrow() {
  return (
    <svg
      width={12}
      height={12}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className="pill-arrow"
    >
      <path d="M3 8h12" pathLength={1} className="pill-arrow-shaft" />
      <path d="M9 4l4 4-4 4" className="pill-arrow-head" />
    </svg>
  )
}

export function ReleasePill() {
  const [paused, setPaused] = useState(false)
  const index = useRotation(registries.length, 4600, paused)
  const registry = registries[index] ?? registries[0]

  return (
    <a
      href={registry.url}
      className="enter pill"
      style={{ '--delay': '0ms' } as React.CSSProperties}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
    >
      <span className="size-1.5 rounded-dot bg-brand" />
      <span>
        v<RollingNumber value={version} /> is on <Swap items={names} index={index} roll className="text-fg" />
      </span>
      <PillArrow />
    </a>
  )
}
