'use client'

import Image from 'next/image'
import Link from 'next/link'
import { useRef, useState } from 'react'
import { ArrowRightIcon, DownloadIcon, Wordmark } from '@/components/icons'
import { logoReferences, logoRounds } from '@/content/logo-concepts'

type Props = {
  image: string
  name: string
  size: number
  wordmark?: boolean
  priority?: boolean
}

function Artwork({ image, name, size, wordmark = false, priority = false }: Props) {
  return (
    <span className={`logo-artwork${wordmark ? ' logo-artwork-lockup' : ''}`}>
      <Image
        src={image}
        alt={`${name} app icon`}
        width={1024}
        height={1024}
        style={{ width: size, height: size }}
        priority={priority}
        draggable={false}
      />
      {wordmark ? <span className="logo-wordmark">kuizer</span> : null}
    </span>
  )
}

export function LogoStudio() {
  const [backdrop, setBackdrop] = useState<'dark' | 'light'>('dark')
  const [presentation, setPresentation] = useState<'icon' | 'lockup'>('icon')
  const [size, setSize] = useState(224)
  const [roundIndex, setRoundIndex] = useState(logoRounds.findIndex((entry) => entry.id === '01'))
  const [selectedId, setSelectedId] = useState<string>('sweep')
  const round = logoRounds[roundIndex] ?? logoRounds[0]
  const concepts = round.concepts
  const selected = concepts.find((concept) => concept.id === selectedId) ?? concepts[0]
  const inspector = useRef<HTMLDialogElement>(null)

  return (
    <div className="logo-studio" data-backdrop={backdrop}>
      <header className="logo-header">
        <Link className="logo-brand" href="/" aria-label="Kuizer home">
          <Wordmark />
        </Link>
        <span className="logo-header-divider" />
        <span className="logo-eyebrow">Design studies</span>
        <Link className="logo-back-link" href="/">
          Back to kuizer <ArrowRightIcon size={14} />
        </Link>
      </header>

      <main className="logo-main">
        <section className="logo-intro" aria-labelledby="logo-heading">
          <div>
            <div className="logo-edition">
              <span /> Identity exploration <span className="logo-edition-separator">/</span> Vol. {round.id}
            </div>
            <h1 id="logo-heading">
              Finding the face
              <br />
              of <span>kuizer.</span>
            </h1>
          </div>
          <div className="logo-intro-aside">
            <p>Exploring character, geometry, and the materials that give an icon its personality.</p>
            <span className="logo-eyebrow">App icons, with character.</span>
          </div>
        </section>

        <section aria-labelledby="logo-directions-heading">
          <fieldset className="logo-round-picker" aria-label="Exploration round">
            {logoRounds.map((entry, index) => (
              <button
                key={entry.id}
                type="button"
                aria-pressed={roundIndex === index}
                onClick={() => {
                  setRoundIndex(index)
                  setSelectedId(entry.concepts[0].id)
                }}
              >
                {entry.name} <span>{String(entry.concepts.length).padStart(2, '0')}</span>
              </button>
            ))}
          </fieldset>
          <div className="logo-toolbar">
            <h2 id="logo-directions-heading" className="logo-eyebrow">
              The explorations <span>{String(concepts.length).padStart(2, '0')}</span>
            </h2>
            <div className="logo-controls">
              <fieldset className="logo-segment" aria-label="Logo presentation">
                <button
                  type="button"
                  aria-pressed={presentation === 'icon'}
                  onClick={() => setPresentation('icon')}
                >
                  Icon
                </button>
                <button
                  type="button"
                  aria-pressed={presentation === 'lockup'}
                  onClick={() => setPresentation('lockup')}
                >
                  With wordmark
                </button>
              </fieldset>
              <div className="logo-control-divider" />
              <label className="logo-size-control">
                <span>Size</span>
                <input
                  aria-label="Preview icon size"
                  type="range"
                  min="144"
                  max="272"
                  step="8"
                  value={size}
                  onChange={(event) => setSize(Number(event.target.value))}
                />
              </label>
              <div className="logo-control-divider" />
              <fieldset className="logo-backdrops" aria-label="Preview backdrop">
                <button
                  className="logo-swatch-dark"
                  type="button"
                  aria-label="Dark backdrop"
                  aria-pressed={backdrop === 'dark'}
                  onClick={() => setBackdrop('dark')}
                />
                <button
                  className="logo-swatch-light"
                  type="button"
                  aria-label="Light backdrop"
                  aria-pressed={backdrop === 'light'}
                  onClick={() => setBackdrop('light')}
                />
              </fieldset>
            </div>
          </div>

          <div className="logo-grid" data-count={concepts.length}>
            {concepts.map((concept) => (
              <article className="logo-concept" key={concept.id} data-selected={selectedId === concept.id}>
                <button
                  className={`logo-canvas logo-canvas-${concept.id}`}
                  type="button"
                  onClick={() => setSelectedId(concept.id)}
                  aria-label={`Select ${concept.name}`}
                  aria-pressed={selectedId === concept.id}
                >
                  <span className="logo-canvas-index">{concept.number} / KU</span>
                  <Artwork
                    image={concept.image}
                    name={concept.name}
                    size={presentation === 'lockup' ? Math.round(size * 0.58) : size}
                    wordmark={presentation === 'lockup'}
                    priority
                  />
                  <span className="logo-canvas-caption">{concept.category}</span>
                  <span className="logo-selection-dot" aria-hidden="true" />
                </button>
                <div className="logo-card-caption">
                  <div>
                    <h3>{concept.name}</h3>
                    <p>{concept.material}</p>
                  </div>
                  <a
                    href={concept.image}
                    download={`kuizer-${concept.id}.png`}
                    className="logo-download-icon"
                    aria-label={`Download ${concept.name} PNG`}
                  >
                    <DownloadIcon size={17} />
                  </a>
                </div>
              </article>
            ))}
          </div>
          <p className="logo-gallery-note">
            <span>Select a direction to take a closer look.</span>
            <span>Original concepts · PNG assets</span>
          </p>
        </section>

        <section className="logo-detail" aria-labelledby="logo-detail-heading">
          <div className={`logo-detail-canvas logo-canvas-${selected.id}`}>
            <span className="logo-eyebrow logo-detail-label">
              In focus <span>/ {selected.number}</span>
            </span>
            <Artwork image={selected.image} name={selected.name} size={336} />
            <button className="logo-enlarge" type="button" onClick={() => inspector.current?.showModal()}>
              View full size <ArrowRightIcon size={14} />
            </button>
          </div>
          <div className="logo-detail-copy">
            <span className="logo-eyebrow">A closer look</span>
            <h2 id="logo-detail-heading">
              {selected.name}
              <span>.</span>
            </h2>
            <p className="logo-detail-description">{selected.description}</p>
            <p className="logo-detail-idea">{selected.idea}</p>
            <div className="logo-palette" aria-label={`${selected.name} color palette`}>
              {selected.colors.map((color) => (
                <span key={color} style={{ backgroundColor: color }} title={color} />
              ))}
              <span className="logo-palette-label">The palette</span>
            </div>
            <a className="logo-download-button" href={selected.image} download={`kuizer-${selected.id}.png`}>
              Download concept <DownloadIcon size={15} />
            </a>
          </div>
        </section>

        <section className="logo-scale-section" aria-labelledby="logo-scale-heading">
          <div className="logo-section-intro">
            <div>
              <span className="logo-eyebrow">The everyday test</span>
              <h2 id="logo-scale-heading">Small, but still kuizer.</h2>
            </div>
            <p>
              The same icon, from a desktop presence
              <br />
              to a tiny corner of your screen.
            </p>
          </div>
          <div className="logo-scale-grid">
            {[128, 64, 32, 16].map((scale) => (
              <div className="logo-scale-item" key={scale}>
                <div>
                  <Artwork image={selected.image} name={selected.name} size={scale} />
                </div>
                <span className="logo-eyebrow">{scale} px</span>
              </div>
            ))}
            <div className="logo-scale-lockup">
              <Artwork image={selected.image} name={selected.name} size={64} wordmark />
              <span className="logo-eyebrow">The signature</span>
            </div>
          </div>
        </section>

        <section className="logo-reference-section" aria-labelledby="logo-reference-heading">
          <div className="logo-reference-copy">
            <span className="logo-eyebrow">On the moodboard</span>
            <h2 id="logo-reference-heading">The craft, not the shape.</h2>
            <p>
              Selected from your icon collection. References for depth, tactile surfaces, and considered
              lighting.
            </p>
          </div>
          <div className="logo-references">
            {logoReferences.map((reference) => (
              <figure key={reference.name}>
                <Image
                  src={reference.image}
                  alt={`${reference.name} style reference`}
                  width={128}
                  height={128}
                />
                <figcaption>
                  <span>{reference.name}</span>
                  <span>{reference.detail}</span>
                </figcaption>
              </figure>
            ))}
          </div>
        </section>
      </main>

      <footer className="logo-footer">
        <span className="logo-brand">
          <Wordmark />
        </span>
        <span>Space for what matters.</span>
        <span className="logo-eyebrow">An identity in progress / 2026</span>
      </footer>

      <dialog ref={inspector} className="logo-inspector" aria-labelledby="logo-inspector-heading">
        <div className="logo-inspector-toolbar">
          <h2 id="logo-inspector-heading">{selected.name}</h2>
          <form method="dialog">
            <button type="submit" autoFocus>
              Close <span aria-hidden="true">×</span>
            </button>
          </form>
        </div>
        <Image
          src={selected.image}
          alt={`${selected.name} full resolution concept`}
          width={1024}
          height={1024}
        />
        <a className="logo-download-button" href={selected.image} download={`kuizer-${selected.id}.png`}>
          Download PNG <DownloadIcon size={15} />
        </a>
      </dialog>
    </div>
  )
}
