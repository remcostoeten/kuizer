export const brandKitUrl = '/brand/kuizer-sweep-brand-kit.zip'
export const brandAssetRoot = '/brand/sweep'

export const brandLogos = [
  {
    name: 'Horizontal, light background',
    file: 'horizontal-light',
    theme: 'light',
    width: 1600,
    height: 480,
  },
  { name: 'Horizontal, dark background', file: 'horizontal-dark', theme: 'dark', width: 1600, height: 480 },
  { name: 'Stacked, light background', file: 'stacked-light', theme: 'light', width: 1000, height: 1000 },
  { name: 'Stacked, dark background', file: 'stacked-dark', theme: 'dark', width: 1000, height: 1000 },
  {
    name: 'Single color, light background',
    file: 'horizontal-mono-light',
    theme: 'light',
    width: 1600,
    height: 480,
  },
  {
    name: 'Single color, dark background',
    file: 'horizontal-mono-dark',
    theme: 'dark',
    width: 1600,
    height: 480,
  },
] as const

export const brandMedia = [
  { name: 'Social preview', file: 'opengraph', width: 1200, height: 630 },
  { name: 'Square post', file: 'square', width: 1200, height: 1200 },
  { name: 'GitHub preview', file: 'github', width: 1280, height: 640 },
  { name: 'Social banner', file: 'banner', width: 1500, height: 500 },
  { name: 'README banner', file: 'readme', width: 1280, height: 320 },
] as const

export const brandIconSizes = [16, 24, 32, 48, 64, 96, 128, 180, 192, 256, 512, 1024] as const

export const brandPalette = [
  { name: 'Ivory', color: '#fff0d6' },
  { name: 'Amber', color: '#ffb36d' },
  { name: 'Orange', color: '#f97844' },
  { name: 'Coral', color: '#c74135' },
  { name: 'Graphite', color: '#171717' },
] as const
