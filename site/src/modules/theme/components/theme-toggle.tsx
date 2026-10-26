'use client'

import { useSyncExternalStore } from 'react'
import { MoonIcon, SunIcon } from '@/shared/icons/icons'
import { readServerTheme, readTheme, subscribeTheme, writeTheme } from '@/shared/helpers/theme'

export function ThemeToggle() {
  const theme = useSyncExternalStore(subscribeTheme, readTheme, readServerTheme)

  return (
    <button
      type="button"
      aria-label={theme === 'light' ? 'Switch to dark theme' : 'Switch to light theme'}
      onClick={() => writeTheme(theme === 'light' ? 'dark' : 'light')}
      className="inline-flex size-8 items-center justify-center text-foreground/40 pressable hover:bg-foreground/5 hover:text-foreground/70 focus-visible:bg-foreground/8 focus-visible:text-foreground"
    >
      {theme === 'light' ? <MoonIcon className="size-4" /> : <SunIcon className="size-4" />}
    </button>
  )
}
