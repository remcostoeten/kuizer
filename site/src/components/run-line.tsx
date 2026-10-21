'use client'

import { useState } from 'react'
import { CopyButton } from '@/components/copy-button'
import { Swap } from '@/components/swap'
import { quickRuns } from '@/content/site'
import { useRotation } from '@/shared/helpers/use-rotation'

export function RunLine() {
  const [paused, setPaused] = useState(false)
  const index = useRotation(quickRuns.length, 5200, paused)
  const command = quickRuns[index] ?? quickRuns[0]

  return (
    <div
      className="inline-flex items-center gap-2 font-mono text-ui text-faint"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
    >
      <span>
        or run <Swap items={quickRuns} index={index} className="text-fg" />
      </span>
      <CopyButton text={command} iconOnly />
    </div>
  )
}
