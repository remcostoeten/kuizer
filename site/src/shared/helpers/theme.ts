import { noop } from '@/shared/helpers/noop'

export type Theme = 'light' | 'dark'

const storageKey = 'kuizer-theme'

/**
 * @name themeScript
 * @description Inline script for the document head that restores a saved light
 * theme before first paint, so the page never flashes dark. Dark also sets a
 * `dark` class, which the docs stylesheet keys on.
 *
 * @example
 * <script dangerouslySetInnerHTML={{ __html: themeScript }} />
 */
export const themeScript = `(function(){var r=document.documentElement,l=false;try{l=localStorage.getItem('${storageKey}')==='light'}catch(e){}if(l){r.setAttribute('data-theme','light')}else{r.classList.add('dark')}})()`

/**
 * @name readTheme
 * @description Reads the active theme from the data-theme attribute on the
 * document element. Dark is the default and carries no attribute.
 *
 * @example
 * const theme = readTheme()
 */
export function readTheme(): Theme {
  return document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark'
}

/**
 * @name readServerTheme
 * @description The theme used during server rendering and hydration, before
 * the document is available.
 *
 * @example
 * useSyncExternalStore(subscribeTheme, readTheme, readServerTheme)
 */
export function readServerTheme(): Theme {
  return 'dark'
}

/**
 * @name subscribeTheme
 * @description Calls back whenever the data-theme attribute changes, so a
 * component can follow the theme through useSyncExternalStore.
 *
 * @example
 * const theme = useSyncExternalStore(subscribeTheme, readTheme, readServerTheme)
 */
export function subscribeTheme(onChange: () => void) {
  const observer = new MutationObserver(onChange)
  observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
  return () => observer.disconnect()
}

function flushStyles(element: HTMLElement) {
  return element.offsetHeight
}

/**
 * @name writeTheme
 * @description Applies a theme to the document, as the data-theme attribute and
 * the `dark` class, and remembers it in localStorage, which the inline script in the root layout reads before paint.
 * Transitions are suspended for the swap so every color changes in the same
 * frame instead of some fading while others snap.
 *
 * @example
 * writeTheme('dark')
 */
export function writeTheme(theme: Theme) {
  const root = document.documentElement
  root.classList.add('theme-switching')
  if (theme === 'light') {
    root.setAttribute('data-theme', 'light')
  } else {
    root.removeAttribute('data-theme')
  }
  root.classList.toggle('dark', theme === 'dark')
  flushStyles(root)
  root.classList.remove('theme-switching')
  try {
    localStorage.setItem(storageKey, theme)
  } catch {
    noop()
  }
}
