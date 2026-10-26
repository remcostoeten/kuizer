import Link from 'next/link'
import { Logo } from '@/components/ui/icons'
import { docsSite } from '@/shared/config/docs-site'
import { MenuToggle } from './menu-toggle'

export function Topbar() {
  return (
    <header className="fixed inset-x-0 top-0 z-50 flex h-(--topbar-height) items-center border-b border-foreground/5 bg-background">
      <Link
        href="/docs"
        className="flex h-full items-center gap-2 px-4 font-mono text-xs tracking-wider uppercase transition-colors duration-150 ease-hover hover:bg-foreground/3 focus-visible:bg-foreground/6 lg:w-(--sidebar-width) lg:border-r lg:border-foreground/5"
      >
        <Logo size={20} />
        {docsSite.name}
        <span className="text-foreground/40">docs</span>
      </Link>
      <Link
        href="/"
        className="ms-4 hidden text-xs text-foreground/50 hover:text-foreground focus-visible:text-foreground lg:inline"
      >
        Home
      </Link>
      <MenuToggle />
    </header>
  )
}
