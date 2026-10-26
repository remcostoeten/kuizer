import type { Metadata } from 'next'
import type { ReactNode } from 'react'
import { metaDescription, siteUrl, tagline } from '@/content/site'
import { brandIcons } from '@/shared/config/brand-icons'
import { inter, plexMono } from '@/shared/fonts'
import { themeScript } from '@/shared/helpers/theme'
import './globals.css'

const title = 'kuizer'

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: {
    default: `${title}: ${tagline}`,
    template: `%s | ${title}`,
  },
  description: metaDescription,
  manifest: '/manifest.webmanifest',
  icons: brandIcons,
  openGraph: {
    type: 'website',
    siteName: title,
    title: `${title}: ${tagline}`,
    description: metaDescription,
    url: '/',
  },
  twitter: {
    card: 'summary_large_image',
    title: `${title}: ${tagline}`,
    description: metaDescription,
    images: ['/brand/sweep/social/opengraph-dark.png'],
  },
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
