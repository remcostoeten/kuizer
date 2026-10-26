import { notFound } from 'next/navigation'
import { renderMarkdown } from '@/modules/docs/server/llms'
import { pageMarkdown } from '@/modules/docs/server/seo'
import { source } from '@/modules/docs/server/source'

export const dynamic = 'force-static'
export const dynamicParams = false

export async function GET(_request: Request, { params }: RouteContext<'/md/docs/[...slug]'>) {
  const { slug } = await params
  const page = source.getPage(slug.slice(0, -1))
  if (!page) notFound()
  return new Response(await renderMarkdown(page), {
    headers: { 'Content-Type': 'text/markdown; charset=utf-8' },
  })
}

export function generateStaticParams() {
  return source.getPages().map((page) => ({ slug: pageMarkdown(page).segments }))
}
