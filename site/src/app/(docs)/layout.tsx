import { RootProvider } from 'fumadocs-ui/provider/next'
import type { Metadata, Viewport } from 'next'
import type { ReactNode } from 'react'
import { DocsSearchDialog } from '@/modules/search/components/search-dialog'
import { searchTags } from '@/modules/search/server/sections'
import { brandIcons } from '@/shared/config/brand-icons'
import { docsSite } from '@/shared/config/docs-site'
import { inter, plexMono } from '@/shared/fonts'
import { IconMotion } from '@/shared/icons/icon-motion'
import { themeScript } from '@/shared/helpers/theme'
import './docs.css'

export const metadata: Metadata = {
  metadataBase: new URL(docsSite.url),
  title: { template: `%s | ${docsSite.name} docs`, default: docsSite.title },
  description: docsSite.description,
  applicationName: docsSite.name,
  keywords: docsSite.keywords,
  authors: [docsSite.author],
  creator: docsSite.author.name,
  manifest: '/manifest.webmanifest',
  icons: brandIcons,
  openGraph: {
    type: 'website',
    siteName: docsSite.title,
    locale: 'en_US',
  },
  twitter: { card: 'summary_large_image' },
  robots: { index: true, follow: true, googleBot: { 'max-image-preview': 'large' } },
}

export const viewport: Viewport = {
  themeColor: '#0f0f0f',
  colorScheme: 'dark light',
}

type Props = {
  children: ReactNode
}

export default function DocsRootLayout({ children }: Props) {
  return (
    <html lang="en" className={`${inter.variable} ${plexMono.variable} antialiased`} suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body className="font-sans">
        <RootProvider
          theme={{ enabled: false }}
          search={{ SearchDialog: DocsSearchDialog, options: { tags: searchTags() } }}
        >
          {children}
        </RootProvider>
        <IconMotion />
      </body>
    </html>
  )
}
