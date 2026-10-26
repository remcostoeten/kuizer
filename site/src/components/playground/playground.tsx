'use client'

import { useEffect, useLayoutEffect, useReducer, useRef, useState, type KeyboardEvent } from 'react'
import { demoCommand, discoveryOrder, scanTotals } from '@/content/demo'
import { TooltipLayer } from '@/components/ui/tooltip-layer'
import { DiskBar } from './disk-bar'
import { initialState, onDiskIds } from './model'
import { reduce } from './reducer'
import { ShellView } from './shell-view'
import { Tui, type Flash } from './tui'

function placeCursor(root: HTMLElement, position: number) {
  const table = root.querySelector<HTMLElement>('[data-table]')
  if (!table) return
  const row = table.querySelector<HTMLElement>(`[data-position="${position}"]`)
  table.style.setProperty('--cursor-y', `${row?.offsetTop ?? 0}px`)
  table.style.setProperty('--cursor-h', `${row?.offsetHeight ?? 0}px`)
  table.toggleAttribute('data-cursor', row !== null)
}

function scrollRowIntoView(root: HTMLElement, position: number) {
  const table = root.querySelector<HTMLElement>('[data-table]')
  const row = root.querySelector<HTMLElement>(`[data-position="${position}"]`)
  if (!table || !row) return
  const header = table.firstElementChild instanceof HTMLElement ? table.firstElementChild.offsetHeight : 0
  const top = row.offsetTop - header
  const bottom = row.offsetTop + row.offsetHeight
  if (top < table.scrollTop) table.scrollTop = top
  else if (bottom > table.scrollTop + table.clientHeight) table.scrollTop = bottom - table.clientHeight
}

export function Playground() {
  const [state, dispatch] = useReducer(reduce, undefined, initialState)
  const [flash, setFlash] = useState<Flash | null>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  const stateRef = useRef(state)
  const scanning = state.screen === 'tui' && !state.scan.finished
  const lastRun = state.runs[state.runs.length - 1]
  const deletingId = lastRun?.ids.find((id) => state.items[id]?.status === 'deleting') ?? null

  useEffect(() => {
    stateRef.current = state
  }, [state])

  useEffect(() => {
    if (state.screen !== 'boot') return
    const done = state.typed >= demoCommand.length
    if (done) {
      const timer = window.setTimeout(() => dispatch({ type: 'launch' }), 450)
      return () => window.clearTimeout(timer)
    }
    const delay = state.typed === 0 ? 900 : 40 + Math.random() * 50
    const timer = window.setTimeout(() => dispatch({ type: 'typed' }), delay)
    return () => window.clearTimeout(timer)
  }, [state.screen, state.typed])

  useEffect(() => {
    if (!scanning) return
    const onDisk = onDiskIds(stateRef.current)
    const ids = discoveryOrder.filter((id) => onDisk.includes(id))
    const total = scanTotals.seconds * 1000
    const started = performance.now()
    const timers = ids.map((id, index) => {
      const at = 320 + ((index + Math.random() * 0.6) / ids.length) * (total - 600)
      return window.setTimeout(() => dispatch({ type: 'reveal', id }), at)
    })
    const ticker = window.setInterval(() => {
      const progress = Math.min((performance.now() - started) / total, 1)
      const eased = 1 - (1 - progress) ** 2
      dispatch({ type: 'progress', dirs: scanTotals.dirs * eased, files: scanTotals.files * eased })
    }, 80)
    timers.push(
      window.setTimeout(() => {
        window.clearInterval(ticker)
        dispatch({ type: 'finish', elapsed: scanTotals.seconds })
      }, total),
    )
    return () => {
      for (const timer of timers) window.clearTimeout(timer)
      window.clearInterval(ticker)
    }
  }, [scanning])

  useEffect(() => {
    if (deletingId === null) return
    const timer = window.setTimeout(() => dispatch({ type: 'deleted', id: deletingId }), 260)
    return () => window.clearTimeout(timer)
  }, [deletingId])

  useEffect(() => {
    const root = rootRef.current
    if (!root || state.screen !== 'tui') return
    const active = document.activeElement
    const inside = active !== null && root.contains(active)
    if (inside || window.matchMedia('(pointer: fine)').matches) root.focus({ preventScroll: true })
  }, [state.screen])

  useEffect(() => {
    if (state.screen === 'tui' && rootRef.current) scrollRowIntoView(rootRef.current, state.cursor)
  }, [state.cursor, state.screen])

  useLayoutEffect(() => {
    if (state.screen === 'tui' && rootRef.current) placeCursor(rootRef.current, state.cursor)
  })

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.metaKey || event.ctrlKey || event.altKey) return
    if (state.screen === 'boot') {
      event.preventDefault()
      dispatch({ type: 'launch' })
      return
    }
    if (state.screen !== 'tui') return
    if (event.key === 'Tab' || /^F\d+$/.test(event.key)) return
    event.preventDefault()
    setFlash({ key: event.key, at: performance.now() })
    dispatch({ type: 'key', key: event.key })
  }

  function onClick() {
    if (state.screen === 'tui') rootRef.current?.focus({ preventScroll: true })
  }

  const title =
    state.screen === 'tui' ? demoCommand : state.screen === 'boot' ? 'shell' : 'shell · q brought you here'

  return (
    <div
      ref={rootRef}
      tabIndex={0}
      role="grid"
      aria-label="kuizer demo. A terminal you can drive with the keyboard."
      onKeyDown={onKeyDown}
      onClick={onClick}
      className="frame term flex min-h-0 flex-1 flex-col font-mono text-terminal transition-colors duration-200 ease-out md:text-terminal-wide"
    >
      <div className="flex shrink-0 items-center gap-2 border-b border-line px-3 py-2">
        <span className="flex gap-1.5">
          <i className="size-2.5 rounded-full bg-traffic-red" />
          <i className="size-2.5 rounded-full bg-traffic-yellow" />
          <i className="size-2.5 rounded-full bg-traffic-green" />
        </span>
        <span className="text-muted ml-2 truncate">{title}</span>
        <span className="text-faint ml-auto hidden whitespace-nowrap text-micro sm:inline">
          {state.screen === 'tui'
            ? 'keyboard works here, and so does the mouse'
            : state.screen === 'shell'
              ? `type kuizer to reopen the list`
              : 'press any key to skip'}
        </span>
      </div>
      <DiskBar state={state} dispatch={dispatch} />
      {state.screen === 'tui' ? (
        <Tui state={state} dispatch={dispatch} flash={flash} />
      ) : (
        <ShellView state={state} dispatch={dispatch} />
      )}
      <TooltipLayer rootRef={rootRef} />
    </div>
  )
}
