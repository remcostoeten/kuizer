import 'server-only'
import { docsSite } from '@/shared/config/docs-site'
import { source } from './source'

type DocsPage = NonNullable<ReturnType<typeof source.getPage>>

const imageFile = 'image.png'
const markdownFile = 'content.md'

/**
 * @name absoluteUrl
 * @description Resolves a site path against the configured site URL.
 *
 * @example
 * absoluteUrl("/docs") // "https://kuizer.app/docs"
 */
export function absoluteUrl(path: string) {
  return new URL(path, docsSite.url).toString()
}

/**
 * @name pageImage
 * @description The Open Graph image route segments and URL for a docs page.
 *
 * @example
 * const { url } = pageImage(page) // "/og/docs/guides/theming/image.png"
 */
export function pageImage(page: DocsPage) {
  const segments = [...page.slugs, imageFile]
  return { segments, url: `/og/docs/${segments.join('/')}` }
}

/**
 * @name pageMarkdown
 * @description The route segments and URL of a docs page rendered as Markdown.
 *
 * @example
 * const { url } = pageMarkdown(page) // "/md/docs/guides/theming/content.md"
 */
export function pageMarkdown(page: DocsPage) {
  const segments = [...page.slugs, markdownFile]
  return { segments, url: `/md/docs/${segments.join('/')}` }
}

/**
 * @name pageBreadcrumbs
 * @description The trail from the docs root to a page, using the titles of the
 * pages it passes through. Folders without an index page are skipped.
 *
 * @example
 * pageBreadcrumbs(page) // [{ name: "Docs", url: "/docs" }, { name: "Install", url: "/docs/install" }]
 */
export function pageBreadcrumbs(page: DocsPage) {
  const trail = [{ name: 'Docs', url: docsSite.basePath }]
  for (let depth = 1; depth <= page.slugs.length; depth++) {
    const ancestor = source.getPage(page.slugs.slice(0, depth))
    if (ancestor) trail.push({ name: ancestor.data.title, url: ancestor.url })
  }
  return trail
}

/**
 * @name pageJsonLd
 * @description Schema.org TechArticle and BreadcrumbList data for a docs page,
 * safe to inline in a script tag.
 *
 * @example
 * <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: pageJsonLd(page) }} />
 */
export function pageJsonLd(page: DocsPage) {
  const data = {
    '@context': 'https://schema.org',
    '@graph': [
      {
        '@type': 'TechArticle',
        '@id': `${absoluteUrl(page.url)}#article`,
        headline: page.data.title,
        description: page.data.description ?? docsSite.description,
        url: absoluteUrl(page.url),
        mainEntityOfPage: absoluteUrl(page.url),
        image: absoluteUrl(pageImage(page).url),
        inLanguage: 'en',
        dateModified: page.data.lastModified?.toISOString(),
        author: { '@type': 'Person', name: docsSite.author.name, url: docsSite.author.url },
        publisher: { '@type': 'Organization', name: docsSite.name, url: docsSite.url },
      },
      {
        '@type': 'BreadcrumbList',
        itemListElement: pageBreadcrumbs(page).map((crumb, index) => ({
          '@type': 'ListItem',
          position: index + 1,
          name: crumb.name,
          item: absoluteUrl(crumb.url),
        })),
      },
    ],
  }
  return JSON.stringify(data).replace(/</g, '\\u003c')
}
