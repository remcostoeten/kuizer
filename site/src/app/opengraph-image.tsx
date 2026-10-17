import { ImageResponse } from 'next/og'
import { description, tagline } from '@/content/site'

export const dynamic = 'force-static'
export const alt = 'kuizer: reclaim your disk from node_modules, target and .next'
export const size = { width: 1200, height: 630 }
export const contentType = 'image/png'

export default function OpenGraphImage() {
  return new ImageResponse(
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'space-between',
        padding: 72,
        background: '#111111',
        color: '#fafafa',
        fontFamily: 'sans-serif',
        backgroundImage: 'radial-gradient(rgba(250,250,249,0.12) 1px, transparent 1px)',
        backgroundSize: '24px 24px',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 16, fontSize: 28 }}>
        <div
          style={{
            width: 36,
            height: 36,
            borderRadius: 9,
            background: '#0c0a09',
            border: '1px solid #292524',
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'center',
            gap: 4,
            padding: 8,
          }}
        >
          <div style={{ height: 3, width: 20, borderRadius: 2, background: '#fafaf9' }} />
          <div style={{ height: 3, width: 14, borderRadius: 2, background: '#a6a09b' }} />
          <div style={{ height: 3, width: 8, borderRadius: 2, background: '#57534e' }} />
        </div>
        <span style={{ fontWeight: 600 }}>kuizer</span>
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 24 }}>
        <div style={{ fontSize: 64, fontWeight: 600, letterSpacing: -2, lineHeight: 1.05, maxWidth: 1000 }}>
          {tagline}
        </div>
        <div style={{ fontSize: 26, color: '#a6a09b', maxWidth: 960, lineHeight: 1.4 }}>{description}</div>
      </div>
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 22, color: '#a6a09b' }}>
        <span style={{ color: '#fafafa', fontWeight: 600 }}>npx kuizer</span>
        <span>Rust, MIT, Linux and macOS</span>
      </div>
    </div>,
    size,
  )
}
