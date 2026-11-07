import type { Metadata } from 'next'

export const brandIcons: NonNullable<Metadata['icons']> = {
  icon: [
    {
      url: '/brand/sweep/favicons/favicon-light.svg',
      type: 'image/svg+xml',
      media: '(prefers-color-scheme: light)',
    },
    {
      url: '/brand/sweep/favicons/favicon-dark.svg',
      type: 'image/svg+xml',
      media: '(prefers-color-scheme: dark)',
    },
    { url: '/brand/sweep/favicons/favicon-48.png', type: 'image/png', sizes: '48x48' },
    { url: '/brand/sweep/icons/app-192.png', type: 'image/png', sizes: '192x192' },
  ],
  apple: [{ url: '/brand/sweep/favicons/apple-touch-icon.png', sizes: '180x180', type: 'image/png' }],
  other: [{ rel: 'mask-icon', url: '/brand/sweep/marks/mono-black.svg', color: '#f97844' }],
}
