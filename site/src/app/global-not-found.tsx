import type { Metadata, Viewport } from 'next'
import Link from 'next/link'
import { Footer } from '@/components/layout/footer'
import { Nav } from '@/components/layout/nav'
import { siteName, siteUrl } from '@/content/site'
import { brandIcons } from '@/shared/config/brand-icons'
import { inter, plexMono } from '@/shared/fonts'
import { themeScript } from '@/shared/helpers/theme'
import './(site)/globals.css'

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: `Page not found | ${siteName}`,
  description: 'There is no page at this address on kuizer.app.',
  icons: brandIcons,
}

export const viewport: Viewport = {
  themeColor: '#0f0f0f',
  colorScheme: 'dark light',
}

export default function GlobalNotFound() {
  return (
    <html lang="en" className={`${inter.variable} ${plexMono.variable}`} suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body className="min-h-dvh antialiased">
        <Nav />
        <main className="wrap flex flex-col items-center pb-32 pt-24 text-center md:pt-32">
          <p className="font-mono text-micro uppercase tracking-brand text-brand">404</p>
          <h1 className="mt-5 text-display-lg font-semibold">Page not found.</h1>
          <p className="mt-5 max-w-reading text-copy text-muted">
            There is no page at this address. The documentation and the download are one click away.
          </p>
          <div className="mt-8 flex flex-wrap items-center justify-center gap-2">
            <Link href="/" className="btn btn-primary">
              Home
            </Link>
            <Link href="/docs" className="btn btn-secondary">
              Documentation
            </Link>
          </div>
        </main>
        <Footer />
      </body>
    </html>
  )
}
