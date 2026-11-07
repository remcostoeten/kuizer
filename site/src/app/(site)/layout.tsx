import type { Metadata, Viewport } from 'next'
import type { ReactNode } from 'react'
import {
  authorName,
  authorUrl,
  keywords,
  metaDescription,
  metaTitle,
  siteName,
  siteUrl,
} from '@/content/site'
import { brandIcons } from '@/shared/config/brand-icons'
import { socialImage } from '@/shared/config/open-graph'
import { inter, plexMono } from '@/shared/fonts'
import { themeScript } from '@/shared/helpers/theme'
import './globals.css'

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: {
    default: metaTitle,
    template: `%s | ${siteName}`,
  },
  description: metaDescription,
  applicationName: siteName,
  authors: [{ name: authorName, url: authorUrl }],
  creator: authorName,
  keywords,
  manifest: '/manifest.webmanifest',
  icons: brandIcons,
  openGraph: { type: 'website', siteName, locale: 'en_US', images: [socialImage] },
  twitter: { card: 'summary_large_image' },
  robots: {
    index: true,
    follow: true,
    googleBot: {
      index: true,
      follow: true,
      'max-image-preview': 'large',
      'max-snippet': -1,
      'max-video-preview': -1,
    },
  },
}

export const viewport: Viewport = {
  themeColor: '#0f0f0f',
  colorScheme: 'dark light',
}

type Props = {
  children: ReactNode
}

export default function RootLayout({ children }: Props) {
  return (
    <html lang="en" className={`${inter.variable} ${plexMono.variable}`} suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body className="min-h-dvh antialiased">{children}</body>
    </html>
  )
}
