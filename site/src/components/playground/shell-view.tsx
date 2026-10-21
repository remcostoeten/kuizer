'use client'

import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { demoCommand } from '@/content/demo'
import type { Action } from './reducer'
import type { ShellTone, State } from './model'

type Props = {
  state: State
  dispatch: (action: Action) => void
}

const toneClass: Record<ShellTone, string> = {
  plain: '',
  muted: 'text-muted',
  warn: 'text-t-yellow',
  ok: 'text-t-green',
  prompt: 'mt-2 font-semibold',
}

export function ShellView({ state, dispatch }: Props) {
  const [value, setValue] = useState('')
  const [historyIndex, setHistoryIndex] = useState<number | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const container = containerRef.current
    if (container) container.scrollTop = container.scrollHeight
  })

  useEffect(() => {
    if (state.screen !== 'shell') return
    if (window.matchMedia('(pointer: fine)').matches) inputRef.current?.focus({ preventScroll: true })
  }, [state.screen])

  function submit() {
    dispatch({ type: 'shell', line: value })
    setValue('')
    setHistoryIndex(null)
  }

  function recall(step: number) {
    const history = state.shellHistory
    if (history.length === 0) return
    const base = historyIndex ?? history.length
    const index = Math.min(Math.max(base + step, 0), history.length)
    setHistoryIndex(index === history.length ? null : index)
    setValue(history[index] ?? '')
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Enter') {
      event.preventDefault()
      submit()
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      recall(-1)
    } else if (event.key === 'ArrowDown') {
      event.preventDefault()
      recall(1)
    } else if (event.key === 'l' && event.ctrlKey) {
      event.preventDefault()
      dispatch({ type: 'shell', line: 'clear' })
    }
  }

  return (
    <div ref={containerRef} className="screen-in min-h-0 flex-1 overflow-y-auto px-3 py-2">
      {state.shell.map((line, index) => (
        <div
          key={`${index}:${line.text}`}
          className={`rise whitespace-pre-wrap break-words ${toneClass[line.tone]}`}
        >
          {line.text === '' ? ' ' : line.text}
        </div>
      ))}
      {state.screen === 'boot' ? (
        <div className="mt-2">
          <span className="text-brand-light">$ </span>
          <span className="caret">{demoCommand.slice(0, state.typed)}</span>
        </div>
      ) : (
        <label className="mt-2 flex items-center">
          <span className="text-brand-light shrink-0">$&nbsp;</span>
          <span className="sr-only">Command</span>
          <input
            ref={inputRef}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            onKeyDown={onKeyDown}
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            enterKeyHint="go"
            placeholder="type help"
            className="min-w-0 flex-1 bg-transparent font-mono text-inherit outline-none placeholder:text-faint"
          />
        </label>
      )}
    </div>
  )
}
