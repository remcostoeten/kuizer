'use client'

import { useEffect, useRef, useState } from 'react'
import { noop } from '@/shared/helpers/noop'

type Props = {
  text: string
  label?: string
  iconOnly?: boolean
  className?: string
}

export function CopyButton({ text, label = 'Copy', iconOnly = false, className = '' }: Props) {
  const [copied, setCopied] = useState(false)
  const timer = useRef<number | null>(null)

  useEffect(() => {
    return () => {
      if (timer.current !== null) window.clearTimeout(timer.current)
    }
  }, [])

  async function copy() {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      if (timer.current !== null) window.clearTimeout(timer.current)
      timer.current = window.setTimeout(() => setCopied(false), 1600)
    } catch {
      noop()
    }
  }

  const base = iconOnly
    ? 'icon-btn'
    : 'press inline-flex h-8 shrink-0 items-center gap-1.5 border border-line px-2.5 text-caption-plain font-medium text-muted hover:border-line-strong hover:text-fg'

  return (
    <button
      type="button"
      onClick={copy}
      aria-live="polite"
      aria-label={copied ? 'Copied' : `${label}: ${text}`}
      title={copied ? 'Copied' : label}
      className={`${base} ${className}`}
    >
      <span key={String(copied)} className="animate-pop-icon inline-flex">
        {copied ? <CheckIcon /> : <ClipboardIcon />}
      </span>
      {iconOnly ? null : <span>{copied ? 'Copied' : label}</span>}
    </button>
  )
}

function ClipboardIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      aria-hidden="true"
    >
      <rect x="5.5" y="5.5" width="8" height="9" rx="1.5" />
      <path d="M10.5 5.5v-2a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v7a1 1 0 0 0 1 1h2" />
    </svg>
  )
}

function CheckIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      aria-hidden="true"
    >
      <path d="M3 8.5l3 3 7-7" />
    </svg>
  )
}
