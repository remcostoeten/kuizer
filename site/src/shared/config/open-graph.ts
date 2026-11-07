import type { Metadata } from 'next'
import { siteName, tagline } from '@/content/site'

export const socialImage = {
  url: '/brand/sweep/social/opengraph-dark.png',
  width: 1200,
  height: 630,
  alt: `${siteName}: ${tagline}`,
}

/**
 * @name pageOpenGraph
 * @description Open Graph data for one site page. A page that sets its own
 * `openGraph` replaces the layout's whole object, so this keeps the shared
 * fields and the social image and adds the page URL. Title and description
 * follow the page's own.
 *
 * @example
 * export const metadata: Metadata = { openGraph: pageOpenGraph('/brand') }
 */
export function pageOpenGraph(path: string): NonNullable<Metadata['openGraph']> {
  return { type: 'website', siteName, locale: 'en_US', url: path, images: [socialImage] }
}
