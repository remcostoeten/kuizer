import { execFileSync } from 'node:child_process'
import type { MetadataRoute } from 'next'
import { siteUrl } from '@/content/site'
import { source } from '@/modules/docs/server/source'
import { noop } from '@/shared/helpers/noop'

export const dynamic = 'force-static'

const sitePages = [
  { path: '/', sources: ['src/app/(site)/page.tsx', 'src/components', 'src/content'] },
  { path: '/brand', sources: ['src/app/(site)/brand/page.tsx', 'src/content/brand.ts'] },
]

function lastCommit(paths: string[]) {
  try {
    const stamp = execFileSync('git', ['log', '-1', '--format=%cI', '--', ...paths], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim()
    return stamp ? new Date(stamp) : undefined
  } catch {
    noop()
    return undefined
  }
}

export default function sitemap(): MetadataRoute.Sitemap {
  const pages = sitePages.map((page) => ({
    url: new URL(page.path, siteUrl).toString(),
    lastModified: lastCommit(page.sources),
  }))
  const docs = source.getPages().map((page) => ({
    url: new URL(page.url, siteUrl).toString(),
    lastModified: page.data.lastModified,
  }))
  return [...pages, ...docs]
}
