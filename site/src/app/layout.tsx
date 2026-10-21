import type { Metadata } from 'next'
import type { ReactNode } from 'react'
import { IBM_Plex_Mono, Inter, Space_Grotesk } from 'next/font/google'
import { description, siteUrl, tagline } from '@/content/site'
import './globals.css'

const inter = Inter({
  subsets: ['latin'],
  axes: ['opsz'],
  variable: '--font-inter',
  display: 'swap',
})

const spaceGrotesk = Space_Grotesk({
  subsets: ['latin'],
  weight: ['500'],
  variable: '--font-space-grotesk',
  display: 'swap',
})

const plexMono = IBM_Plex_Mono({
  subsets: ['latin'],
  weight: ['400', '500', '600'],
  variable: '--font-plex-mono',
  display: 'swap',
})

const title = 'kuizer'

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: {
    default: `${title}: ${tagline}`,
    template: `%s | ${title}`,
  },
  description,
  manifest: '/manifest.webmanifest',
  icons: {
    icon: [
      {
        url: '/brand/sweep/favicons/favicon-light.svg',
        type: 'image/svg+xml',
        media: '(prefers-color-scheme: light)',
      },
      {
        url: '/brand/sweep/favicons/favicon-dark.svg',
        type: 'image/svg+xml',
        media: '(prefers-color-scheme: dark)',
      },
    ],
    other: [{ rel: 'mask-icon', url: '/brand/sweep/marks/mono-black.svg', color: '#f97844' }],
  },
  openGraph: {
    type: 'website',
    siteName: title,
    title: `${title}: ${tagline}`,
    description,
    url: '/',
  },
  twitter: {
    card: 'summary_large_image',
    title: `${title}: ${tagline}`,
    description,
    images: ['/brand/sweep/social/opengraph-dark.png'],
  },
}

const themeScript = `(function(){try{if(localStorage.getItem('kuizer-theme')==='light'){document.documentElement.setAttribute('data-theme','light')}}catch(e){}})()`

type Props = {
  children: ReactNode
}

export default function RootLayout({ children }: Props) {
  return (
    <html
      lang="en"
      className={`${inter.variable} ${spaceGrotesk.variable} ${plexMono.variable}`}
      suppressHydrationWarning
    >
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body className="min-h-dvh antialiased">{children}</body>
    </html>
  )
}
