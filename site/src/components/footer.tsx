import { GithubIcon, Wordmark } from '@/components/icons'
import {
  authorUrl,
  configDocsUrl,
  cratesUrl,
  designDocsUrl,
  docsUrl,
  issuesUrl,
  licenseUrl,
  npmUrl,
  releasesUrl,
  repoUrl,
} from '@/content/site'

const columns = [
  {
    title: 'Project',
    links: [
      { label: 'Releases', href: releasesUrl },
      { label: 'npm', href: npmUrl },
      { label: 'crates.io', href: cratesUrl },
      { label: 'Issues', href: issuesUrl },
    ],
  },
  {
    title: 'Docs',
    links: [
      { label: 'README', href: docsUrl },
      { label: 'Config keys', href: configDocsUrl },
      { label: 'Design notes', href: designDocsUrl },
      { label: 'Brand assets', href: '/brand' },
    ],
  },
  {
    title: 'Legal',
    links: [{ label: 'MIT license', href: licenseUrl }],
  },
]

export function Footer() {
  return (
    <footer className="border-t border-line">
      <div className="wrap grid gap-10 py-14 md:footer-grid">
        <div className="max-w-footer">
          <Wordmark />
          <p className="mt-4 text-copy-sm text-faint">
            Finds build output, development caches and duplicate files, and removes the ones you select. Runs
            in the terminal and as a desktop app.
          </p>
          <a href={repoUrl} aria-label="kuizer on GitHub" className="icon-btn mt-5 -ml-1.5 inline-flex">
            <GithubIcon size={16} />
          </a>
        </div>
        <div className="grid grid-cols-2 gap-10 sm:grid-cols-3 md:gap-16">
          {columns.map((column) => (
            <div key={column.title}>
              <h3 className="text-ui font-medium">{column.title}</h3>
              <ul className="mt-3 space-y-2">
                {column.links.map((link) => (
                  <li key={link.href}>
                    <a
                      href={link.href}
                      className="text-ui text-faint transition-colors hover:text-fg focus-visible:text-fg focus-visible:transition-none"
                    >
                      {link.label}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      </div>
      <div className="wrap flex flex-wrap items-center justify-between gap-3 border-t border-line py-5 font-mono text-micro text-faint">
        <span>
          xxx,{' '}
          <a
            href={authorUrl}
            className="transition-colors hover:text-fg focus-visible:text-fg focus-visible:transition-none"
          >
            Remco Stoeten
          </a>
        </span>
        <span>MIT</span>
      </div>
    </footer>
  )
}
