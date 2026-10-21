'use client'

import type { KeyboardEvent } from 'react'
import { useId, useRef, useState } from 'react'
import { CopyButton } from '@/components/copy-button'
import { installOptions } from '@/content/install'

export function InstallBox() {
  const [activeId, setActiveId] = useState(installOptions[0]?.id ?? '')
  const baseId = useId()
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([])

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

  return (
    <div className="panel w-full max-w-copy text-left">
      <div
        role="tablist"
        aria-label="Install method"
        className="flex overflow-x-auto border-b border-line px-1"
      >
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
            aria-controls={`${baseId}-panel-${option.id}`}
            tabIndex={option.id === activeId ? 0 : -1}
            onClick={() => setActiveId(option.id)}
            onKeyDown={(event) => onKeyDown(event, index)}
            className="install-tab shrink-0"
          >
            {option.label}
          </button>
        ))}
      </div>
      <div className="grid">
        {installOptions.map((option) => (
          <div
            key={option.id}
            role="tabpanel"
            id={`${baseId}-panel-${option.id}`}
            aria-labelledby={`${baseId}-tab-${option.id}`}
            className={`col-start-1 row-start-1 px-4 py-4 ${option.id === activeId ? 'rise' : 'invisible'}`}
          >
            <div className="flex items-start gap-3">
              <code className="min-w-0 flex-1 text-ui leading-6 break-all">
                <span className="text-faint select-none">$ </span>
                {option.command}
              </code>
              <CopyButton text={option.command} iconOnly className="-mt-0.5 -mr-1.5 shrink-0" />
            </div>
            <p className="mt-2 text-caption-plain text-faint">{option.note}</p>
          </div>
        ))}
      </div>
    </div>
  )
}
