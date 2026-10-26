import type { MetadataRoute } from 'next'

export const dynamic = 'force-static'

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: 'kuizer',
    short_name: 'kuizer',
    description: 'Find development caches, build output and duplicate files.',
    start_url: '/',
    display: 'browser',
    background_color: '#0f0f0f',
    theme_color: '#0f0f0f',
    icons: [
      { src: '/brand/sweep/icons/app-192.png', sizes: '192x192', type: 'image/png', purpose: 'any' },
      { src: '/brand/sweep/icons/app-512.png', sizes: '512x512', type: 'image/png', purpose: 'any' },
      {
        src: '/brand/sweep/icons/maskable-192.png',
        sizes: '192x192',
        type: 'image/png',
        purpose: 'maskable',
      },
      {
        src: '/brand/sweep/icons/maskable-512.png',
        sizes: '512x512',
        type: 'image/png',
        purpose: 'maskable',
      },
    ],
  }
}
