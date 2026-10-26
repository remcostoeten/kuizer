import type { Metadata } from 'next'
import Image from 'next/image'
import { Footer } from '@/components/layout/footer'
import { DownloadIcon } from '@/components/ui/icons'
import { Nav } from '@/components/layout/nav'
import {
  brandAssetRoot,
  brandIconSizes,
  brandKitUrl,
  brandLogos,
  brandMedia,
  brandPalette,
} from '@/content/brand'

export const metadata: Metadata = {
  title: 'Brand assets',
  alternates: { canonical: '/brand' },
  description:
    'The kuizer Sweep logo, app icons, wordmarks and media assets in color and single-color formats.',
}

export default function BrandPage() {
  return (
    <>
      <Nav />
      <main className="wrap pb-24 pt-16 md:pt-24">
        <header className="grid items-center gap-10 border-b border-line pb-16 md:grid-cols-[1fr_240px]">
          <div>
            <p className="font-mono text-micro uppercase tracking-brand text-brand">Brand assets / Sweep</p>
            <h1 className="mt-5 text-display-lg font-semibold">The kuizer identity.</h1>
            <p className="mt-5 max-w-reading text-copy text-muted">
              Sweep is the kuizer logo. Download the app icon, transparent marks, logos and media exports for
              light and dark backgrounds.
            </p>
            <a href={brandKitUrl} download className="btn btn-primary mt-8">
              <DownloadIcon size={18} /> Download the complete kit
            </a>
          </div>
          <Image
            src={`${brandAssetRoot}/icons/app-512.png`}
            alt="The selected Sweep app icon"
            width={240}
            height={240}
            priority
          />
        </header>

        <section className="border-b border-line py-14" aria-labelledby="brand-marks">
          <h2 id="brand-marks" className="text-section font-medium">
            The mark
          </h2>
          <p className="mt-3 max-w-reading text-copy-sm text-muted">
            The original app tile and a detached mark for layouts. Use the single-color SVG at small sizes or
            when color is unavailable.
          </p>
          <div className="mt-8 grid gap-6 sm:grid-cols-2 lg:grid-cols-4">
            {[
              { name: 'App icon', path: 'icons/app-1024.png', background: '#fff8ef' },
              { name: 'Transparent color mark', path: 'marks/full-color.png', background: '#171717' },
              { name: 'Black vector mark', path: 'marks/mono-black.svg', background: '#fff8ef' },
              { name: 'White vector mark', path: 'marks/mono-white.svg', background: '#171717' },
            ].map((asset) => (
              <figure key={asset.path}>
                <div
                  className="flex aspect-square items-center justify-center p-8"
                  style={{ backgroundColor: asset.background }}
                >
                  <Image
                    src={`${brandAssetRoot}/${asset.path}`}
                    alt={asset.name}
                    width={220}
                    height={220}
                    className="h-auto w-full"
                  />
                </div>
                <figcaption className="mt-4 text-ui">
                  <a
                    href={`${brandAssetRoot}/${asset.path}`}
                    download
                    className="text-muted hover:text-fg focus-visible:text-brand"
                  >
                    {asset.name} ↓
                  </a>
                </figcaption>
              </figure>
            ))}
          </div>
        </section>

        <section className="border-b border-line py-14" aria-labelledby="brand-logos">
          <h2 id="brand-logos" className="text-section font-medium">
            Logos and wordmarks
          </h2>
          <p className="mt-3 max-w-reading text-copy-sm text-muted">
            Horizontal and stacked layouts with outlined IBM Plex Mono lettering. PNGs have transparent
            backgrounds. Color SVGs embed the raster mark; single-color SVGs contain vector paths.
          </p>
          <div className="mt-8 grid gap-x-6 gap-y-8 md:grid-cols-2">
            {brandLogos.map((asset) => (
              <figure key={asset.file}>
                <div
                  className="flex h-56 items-center justify-center px-8 py-6"
                  style={{ backgroundColor: asset.theme === 'light' ? '#fff8ef' : '#171717' }}
                >
                  <Image
                    src={`${brandAssetRoot}/logos/${asset.file}.svg`}
                    alt={asset.name}
                    width={asset.width}
                    height={asset.height}
                    className="max-h-full w-full object-contain"
                  />
                </div>
                <figcaption className="mt-4 flex flex-wrap items-center justify-between gap-3 text-ui">
                  <span>{asset.name}</span>
                  <span className="flex gap-4 text-muted">
                    {['svg', 'png'].map((format) => (
                      <a
                        key={format}
                        href={`${brandAssetRoot}/logos/${asset.file}.${format}`}
                        download
                        className="uppercase hover:text-fg focus-visible:text-brand"
                      >
                        {format}
                      </a>
                    ))}
                  </span>
                </figcaption>
              </figure>
            ))}
          </div>
          <div className="mt-8 flex flex-wrap gap-6 text-ui text-muted">
            {['black', 'white', 'orange'].map((color) => (
              <a
                key={color}
                href={`${brandAssetRoot}/wordmarks/${color}.svg`}
                download
                className="hover:text-fg focus-visible:text-brand"
              >
                {color[0]?.toUpperCase()}
                {color.slice(1)} wordmark SVG ↓
              </a>
            ))}
          </div>
        </section>

        <section className="border-b border-line py-14" aria-labelledby="brand-icons">
          <h2 id="brand-icons" className="text-section font-medium">
            App icons and favicons
          </h2>
          <div className="mt-8 flex flex-wrap items-end gap-8">
            {[32, 64, 128].map((size) => (
              <figure key={size} className="text-center">
                <Image
                  src={`${brandAssetRoot}/icons/app-${size}.png`}
                  alt={`Sweep at ${size} pixels`}
                  width={size}
                  height={size}
                />
                <figcaption className="mt-3 font-mono text-micro text-faint">{size} px</figcaption>
              </figure>
            ))}
          </div>
          <div className="mt-8 flex flex-wrap gap-x-6 gap-y-4 text-ui text-muted">
            {brandIconSizes.map((size) => (
              <a
                key={size}
                href={`${brandAssetRoot}/icons/app-${size}.png`}
                download
                className="hover:text-fg focus-visible:text-brand"
              >
                {size} px PNG
              </a>
            ))}
          </div>
          <div className="mt-6 flex flex-wrap gap-6 text-ui text-muted">
            {[
              ['macOS ICNS', 'icons/kuizer.icns'],
              ['Favicon ICO', 'favicons/favicon.ico'],
              ['Apple touch icon', 'favicons/apple-touch-icon.png'],
              ['Maskable web icon', 'icons/maskable-512.png'],
              ['Dark theme favicon', 'favicons/favicon-dark.svg'],
              ['Light theme favicon', 'favicons/favicon-light.svg'],
            ].map(([name, path]) => (
              <a
                key={path}
                href={`${brandAssetRoot}/${path}`}
                download
                className="hover:text-fg focus-visible:text-brand"
              >
                {name} ↓
              </a>
            ))}
          </div>
        </section>

        <section className="border-b border-line py-14" aria-labelledby="brand-media">
          <h2 id="brand-media" className="text-section font-medium">
            Social and media
          </h2>
          <p className="mt-3 text-copy-sm text-muted">
            Each composition comes in light and dark versions, as PNG and JPG.
          </p>
          <Image
            src={`${brandAssetRoot}/social/opengraph-dark.png`}
            alt="kuizer social preview with the Sweep logo and Reclaim your disk headline"
            width={1200}
            height={630}
            className="mt-8 h-auto w-full border border-line"
          />
          <div className="mt-8 divide-y divide-line">
            {brandMedia.map((asset) => (
              <div
                key={asset.file}
                className="flex flex-wrap items-center justify-between gap-4 py-5 text-ui"
              >
                <div>
                  <h3>{asset.name}</h3>
                  <p className="mt-1 font-mono text-micro text-faint">
                    {asset.width} × {asset.height}
                  </p>
                </div>
                <div className="flex flex-wrap gap-4 text-muted">
                  {['light', 'dark'].map((theme) =>
                    ['png', 'jpg'].map((format) => (
                      <a
                        key={`${theme}-${format}`}
                        href={`${brandAssetRoot}/social/${asset.file}-${theme}.${format}`}
                        download
                        className="hover:text-fg focus-visible:text-brand"
                      >
                        {theme === 'light' ? 'Light' : 'Dark'} {format.toUpperCase()}
                      </a>
                    )),
                  )}
                </div>
              </div>
            ))}
          </div>
          <div className="mt-6 flex flex-wrap gap-6 text-ui text-muted">
            {['light', 'dark'].map((theme) => (
              <a
                key={theme}
                href={`${brandAssetRoot}/email/signature-${theme}.png`}
                download
                className="hover:text-fg focus-visible:text-brand"
              >
                {theme === 'light' ? 'Light' : 'Dark'} email signature ↓
              </a>
            ))}
          </div>
        </section>

        <section className="py-14" aria-labelledby="brand-palette">
          <h2 id="brand-palette" className="text-section font-medium">
            Palette and use
          </h2>
          <div className="mt-8 grid grid-cols-2 gap-4 sm:grid-cols-5">
            {brandPalette.map((entry) => (
              <div key={entry.color}>
                <div className="h-20 border border-line" style={{ backgroundColor: entry.color }} />
                <p className="mt-3 text-ui">{entry.name}</p>
                <p className="mt-1 font-mono text-micro text-faint">{entry.color}</p>
              </div>
            ))}
          </div>
          <p className="mt-8 max-w-reading text-copy-sm text-muted">
            Keep the ribbon and three blocks together. Preserve the proportions and leave at least one block
            of clear space around the mark. The original full-color tile is included unchanged.
          </p>
          <div className="mt-6 flex gap-6 text-ui text-muted">
            <a
              href={`${brandAssetRoot}/USAGE.md`}
              download
              className="hover:text-fg focus-visible:text-brand"
            >
              Usage notes ↓
            </a>
            <a
              href={`${brandAssetRoot}/manifest.json`}
              download
              className="hover:text-fg focus-visible:text-brand"
            >
              File manifest ↓
            </a>
          </div>
        </section>
      </main>
      <Footer />
    </>
  )
}
