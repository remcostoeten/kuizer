import { DocsBody, DocsDescription, DocsPage, DocsTitle } from 'fumadocs-ui/page'
import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { PageActions } from '@/modules/docs/components/page-actions'
import { PageMeta } from '@/modules/docs/components/page-meta'
import { mdxComponents } from '@/modules/docs/mdx-components'
import { pageImage, pageJsonLd, pageMarkdown } from '@/modules/docs/server/seo'
import { source } from '@/modules/docs/server/source'
import { docsSite } from '@/shared/config/docs-site'

type Props = {
  params: Promise<{ slug?: string[] }>
}

export const dynamicParams = false

export default async function Page({ params }: Props) {
  const { slug } = await params
  const page = source.getPage(slug)
  if (!page) notFound()

  const MDX = page.data.body
  const { repository } = docsSite
  const editUrl = `${repository.url}/blob/${repository.branch}/${repository.contentDir}/${page.path}`

  return (
    <DocsPage toc={page.data.toc} tableOfContent={{ style: 'clerk' }} breadcrumb={{ enabled: false }}>
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: pageJsonLd(page) }} />
      <DocsTitle>{page.data.title}</DocsTitle>
      {page.data.description ? <DocsDescription>{page.data.description}</DocsDescription> : null}
      <PageActions markdownUrl={pageMarkdown(page).url} />
      <DocsBody>
        <MDX components={mdxComponents} />
      </DocsBody>
      <PageMeta editUrl={editUrl} lastModified={page.data.lastModified} />
    </DocsPage>
  )
}

export function generateStaticParams() {
  return source.generateParams()
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { slug } = await params
  const page = source.getPage(slug)
  if (!page) notFound()
  const image = pageImage(page).url
  return {
    title: page.url === docsSite.basePath ? { absolute: docsSite.title } : page.data.title,
    description: page.data.description,
    alternates: {
      canonical: page.url,
      types: { 'text/markdown': pageMarkdown(page).url },
    },
    openGraph: {
      type: 'article',
      siteName: docsSite.title,
      locale: 'en_US',
      title: page.data.title,
      description: page.data.description,
      url: page.url,
      images: [{ url: image, width: 1200, height: 630, alt: page.data.title }],
      modifiedTime: page.data.lastModified?.toISOString(),
    },
    twitter: {
      card: 'summary_large_image',
      title: page.data.title,
      description: page.data.description,
      images: image,
    },
  }
}
