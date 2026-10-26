'use client'

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react'
import { createPortal } from 'react-dom'

type Props = {
  rootRef: RefObject<HTMLElement | null>
}

type Tip = {
  target: HTMLElement
  warm: boolean
  session: number
}

const showDelay = 320
const warmWindow = 260
const hideDelay = 70
const gap = 7
const margin = 8
const arrowInset = 10

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high)
}

function tipTarget(node: EventTarget | null, root: HTMLElement): HTMLElement | null {
  if (!(node instanceof Element)) return null
  const found = node.closest<HTMLElement>('[data-tip]')
  return found !== null && root.contains(found) ? found : null
}

export function TooltipLayer({ rootRef }: Props) {
  const [tip, setTip] = useState<Tip | null>(null)
  const bubbleRef = useRef<HTMLDivElement>(null)
  const timer = useRef(0)
  const visible = useRef(false)
  const hiddenAt = useRef(-Infinity)
  const session = useRef(0)

  const drop = useCallback(() => {
    visible.current = false
    hiddenAt.current = performance.now()
    setTip(null)
  }, [])

  const attach = useCallback(
    (root: HTMLElement) => {
      function show(target: HTMLElement) {
        window.clearTimeout(timer.current)
        const warm = visible.current
        if (warm || performance.now() - hiddenAt.current < warmWindow) {
          if (!warm) session.current += 1
          visible.current = true
          setTip({ target, warm, session: session.current })
          return
        }
        timer.current = window.setTimeout(() => {
          session.current += 1
          visible.current = true
          setTip({ target, warm: false, session: session.current })
        }, showDelay)
      }

      function hide(now = false) {
        window.clearTimeout(timer.current)
        if (!visible.current) return
        if (now) drop()
        else timer.current = window.setTimeout(drop, hideDelay)
      }

      function onPointerOver(event: PointerEvent) {
        if (event.pointerType === 'touch') return
        const target = tipTarget(event.target, root)
        if (target === null) hide()
        else show(target)
      }

      function onPointerLeave() {
        hide()
      }

      function onFocusIn(event: FocusEvent) {
        const target = tipTarget(event.target, root)
        if (target !== null && target.matches(':focus-visible')) show(target)
      }

      function onFocusOut(event: FocusEvent) {
        if (tipTarget(event.target, root) !== null) hide()
      }

      function onDismiss() {
        hide(true)
      }

      root.addEventListener('pointerover', onPointerOver)
      root.addEventListener('pointerleave', onPointerLeave)
      root.addEventListener('pointerdown', onDismiss)
      root.addEventListener('focusin', onFocusIn)
      root.addEventListener('focusout', onFocusOut)
      root.addEventListener('keydown', onDismiss, true)
      root.addEventListener('scroll', onDismiss, true)
      window.addEventListener('scroll', onDismiss, true)
      return () => {
        window.clearTimeout(timer.current)
        root.removeEventListener('pointerover', onPointerOver)
        root.removeEventListener('pointerleave', onPointerLeave)
        root.removeEventListener('pointerdown', onDismiss)
        root.removeEventListener('focusin', onFocusIn)
        root.removeEventListener('focusout', onFocusOut)
        root.removeEventListener('keydown', onDismiss, true)
        root.removeEventListener('scroll', onDismiss, true)
        window.removeEventListener('scroll', onDismiss, true)
      }
    },
    [drop],
  )

  useEffect(() => {
    const root = rootRef.current
    return root ? attach(root) : undefined
  }, [attach, rootRef])

  useLayoutEffect(() => {
    const bubble = bubbleRef.current
    if (!tip || !bubble) return
    const text = tip.target.getAttribute('data-tip') ?? ''
    if (!tip.target.isConnected || text === '') {
      bubble.hidden = true
      window.clearTimeout(timer.current)
      timer.current = window.setTimeout(drop, 0)
      return
    }
    if (bubble.textContent !== text) bubble.textContent = text
    const rect = tip.target.getBoundingClientRect()
    const width = bubble.offsetWidth
    const height = bubble.offsetHeight
    const centre = rect.left + rect.width / 2
    const x = clamp(centre - width / 2, margin, window.innerWidth - margin - width)
    const above = rect.top - gap - height >= margin
    const y = above ? rect.top - gap - height : rect.bottom + gap
    bubble.style.translate = `${Math.round(x)}px ${Math.round(y)}px`
    bubble.style.setProperty(
      '--arrow-x',
      `${Math.round(clamp(centre - x, arrowInset, width - arrowInset))}px`,
    )
    bubble.dataset.side = above ? 'top' : 'bottom'
  })

  if (!tip) return null
  return createPortal(
    <div
      key={tip.session}
      ref={bubbleRef}
      role="tooltip"
      className="tip"
      data-warm={tip.warm ? '' : undefined}
    />,
    document.body,
  )
}
