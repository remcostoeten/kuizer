'use client'

import { useSyncExternalStore } from 'react'
import { readServerTheme, readTheme, subscribeTheme, writeTheme } from '@/shared/helpers/theme'

export function ThemeToggle() {
  const theme = useSyncExternalStore(subscribeTheme, readTheme, readServerTheme)
  const label = theme === 'light' ? 'Switch to dark theme' : 'Switch to light theme'

  return (
    <button
      type="button"
      onClick={() => writeTheme(theme === 'light' ? 'dark' : 'light')}
      aria-label={label}
      title={label}
      className="press inline-flex size-9 items-center justify-center text-faint hover:bg-inset hover:text-fg focus-visible:bg-inset focus-visible:text-brand focus-visible:transition-none"
    >
      <span key={theme} className="animate-pop-icon inline-flex">
        {theme === 'light' ? <MoonIcon /> : <SunIcon />}
      </span>
    </button>
  )
}

function SunIcon() {
  return (
    <svg
      width="15"
      height="15"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      aria-hidden="true"
    >
      <circle cx="8" cy="8" r="3" />
      <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" />
    </svg>
  )
}

function MoonIcon() {
  return (
    <svg
      width="15"
      height="15"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      aria-hidden="true"
    >
      <path d="M13.5 10.2A6 6 0 0 1 5.8 2.5a6 6 0 1 0 7.7 7.7z" />
    </svg>
  )
}
