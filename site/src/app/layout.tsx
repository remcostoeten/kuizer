import type { Metadata } from 'next'
import type { ReactNode } from 'react'
import { IBM_Plex_Mono, Inter } from 'next/font/google'
import { description, siteUrl, tagline } from '@/content/site'
import './globals.css'

const inter = Inter({
  subsets: ['latin'],
  axes: ['opsz'],
  variable: '--font-inter',
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
  },
}

const themeScript = `(function(){try{if(localStorage.getItem('kuizer-theme')==='light'){document.documentElement.setAttribute('data-theme','light')}}catch(e){}})()`

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
