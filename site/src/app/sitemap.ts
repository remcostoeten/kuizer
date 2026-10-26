import type { MetadataRoute } from 'next'
import { siteUrl } from '@/content/site'
import { source } from '@/modules/docs/server/source'

export const dynamic = 'force-static'

export default function sitemap(): MetadataRoute.Sitemap {
  const pages = source.getPages().map((page) => ({
    url: new URL(page.url, siteUrl).toString(),
    lastModified: page.data.lastModified,
    changeFrequency: 'weekly' as const,
    priority: page.slugs.length === 0 ? 0.9 : 0.7,
  }))
  return [{ url: new URL('/', siteUrl).toString(), changeFrequency: 'weekly', priority: 1 }, ...pages]
}
