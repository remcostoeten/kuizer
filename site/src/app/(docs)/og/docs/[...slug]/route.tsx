import { notFound } from 'next/navigation'
import { ImageResponse } from 'next/og'
import { pageImage } from '@/modules/docs/server/seo'
import { source } from '@/modules/docs/server/source'
import { docsSite } from '@/shared/config/docs-site'

export const dynamic = 'force-static'
export const dynamicParams = false

export async function GET(_request: Request, { params }: RouteContext<'/og/docs/[...slug]'>) {
  const { slug } = await params
  const page = source.getPage(slug.slice(0, -1))
  if (!page) notFound()

  return new ImageResponse(
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'space-between',
        width: '100%',
        height: '100%',
        padding: '72px 80px',
        background: '#0f0f0f',
        color: '#e6e6e6',
        fontFamily: 'monospace',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 16,
          fontSize: 24,
          letterSpacing: 4,
          textTransform: 'uppercase',
          color: '#8c8c8c',
        }}
      >
        <div style={{ width: 14, height: 14, background: '#d25611' }} />
        {docsSite.name} docs
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 24 }}>
        <div style={{ fontSize: 72, fontWeight: 700, lineHeight: 1.05, letterSpacing: -2 }}>
          {page.data.title}
        </div>
        {page.data.description ? (
          <div style={{ fontSize: 30, lineHeight: 1.4, color: '#8c8c8c', maxWidth: 900 }}>
            {page.data.description}
          </div>
        ) : null}
      </div>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          borderTop: '1px solid #262626',
          paddingTop: 28,
          fontSize: 22,
          color: '#6b6b6b',
        }}
      >
        <span>{new URL(docsSite.url).host}</span>
        <span>{page.url}</span>
      </div>
    </div>,
    { width: 1200, height: 630 },
  )
}

export function generateStaticParams() {
  return source.getPages().map((page) => ({ slug: pageImage(page).segments }))
}
