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
  ],
  other: [{ rel: 'mask-icon', url: '/brand/sweep/marks/mono-black.svg', color: '#f97844' }],
}
