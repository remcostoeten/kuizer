import Link from 'next/link'
import { DownloadButton } from '@/components/ui/download-button'
import { GithubIcon, Wordmark } from '@/components/ui/icons'
import { ThemeToggle } from '@/components/ui/theme-toggle'
import { cratesUrl, npmUrl, releasesUrl, repoUrl } from '@/content/site'

const links = [
  { label: 'Docs', href: '/docs' },
  { label: 'Releases', href: releasesUrl },
  { label: 'npm', href: npmUrl },
  { label: 'crates.io', href: cratesUrl },
]

export function Nav() {
  return (
    <header className="sticky top-0 z-30 bg-nav backdrop-blur-md">
      <div className="flex h-16 w-full items-center justify-between px-6">
        <Link href="/" aria-label="kuizer home" className="flex items-center focus-visible:text-brand">
          <Wordmark />
        </Link>
        <nav aria-label="Primary" className="flex items-center gap-1">
          {links.map((link) => (
            <a
              key={link.href}
              href={link.href}
              className="hidden px-3 py-2 text-ui text-muted transition-colors duration-200 hover:text-fg focus-visible:text-fg focus-visible:transition-none md:inline-flex"
            >
              {link.label}
            </a>
          ))}
          <a
            href={repoUrl}
            className="hidden items-center gap-1.5 px-3 py-2 text-ui text-muted transition-colors duration-200 hover:text-fg focus-visible:text-fg focus-visible:transition-none sm:inline-flex"
          >
            <GithubIcon size={14} />
            GitHub
          </a>
          <ThemeToggle />
          <DownloadButton size="sm" short />
        </nav>
      </div>
    </header>
  )
}
