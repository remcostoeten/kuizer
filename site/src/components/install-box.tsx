'use client'

import type { KeyboardEvent } from 'react'
import { useId, useRef, useState } from 'react'
import { CopyButton } from '@/components/copy-button'
import { installOptions } from '@/content/install'

export function InstallBox() {
  const [activeId, setActiveId] = useState(installOptions[0]?.id ?? '')
  const baseId = useId()
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([])
  const active = installOptions.find((option) => option.id === activeId) ?? installOptions[0]

  function activate(index: number) {
    const option = installOptions[index]
    if (!option) return
    setActiveId(option.id)
    tabRefs.current[index]?.focus()
  }

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const last = installOptions.length - 1
    const keys: Record<string, number> = {
      ArrowRight: index === last ? 0 : index + 1,
      ArrowLeft: index === 0 ? last : index - 1,
      Home: 0,
      End: last,
    }
    const next = keys[event.key]
    if (next === undefined) return
    event.preventDefault()
    activate(next)
  }

  if (!active) return null

  return (
    <div className="panel w-full max-w-copy text-left">
      <div className="flex items-center justify-between border-b border-line pl-1 pr-1.5">
        <div role="tablist" aria-label="Install method" className="flex overflow-x-auto">
          {installOptions.map((option, index) => (
            <button
              key={option.id}
              ref={(node) => {
                tabRefs.current[index] = node
              }}
              role="tab"
              type="button"
              id={`${baseId}-tab-${option.id}`}
              aria-selected={option.id === activeId}
              aria-controls={`${baseId}-panel`}
              tabIndex={option.id === activeId ? 0 : -1}
              onClick={() => setActiveId(option.id)}
              onKeyDown={(event) => onKeyDown(event, index)}
              className="install-tab shrink-0"
            >
              {option.label}
            </button>
          ))}
        </div>
        <CopyButton text={active.command} iconOnly />
      </div>
      <div
        role="tabpanel"
        id={`${baseId}-panel`}
        aria-labelledby={`${baseId}-tab-${active.id}`}
        className="px-4 py-3.5"
      >
        <code className="block overflow-x-auto whitespace-nowrap text-ui">
          <span className="text-faint">$ </span>
          {active.command}
        </code>
        <p className="mt-2 text-caption-plain text-faint">{active.note}</p>
      </div>
    </div>
  )
}
