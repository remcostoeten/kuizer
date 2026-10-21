'use client'

import Image from 'next/image'
import Link from 'next/link'
import { useRef, useState } from 'react'
import { Wordmark } from '@/components/icons'
import { concepts } from './concepts'

export default function LogoLab() {
  const [selectedId, setSelectedId] = useState('clear')
  const [surface, setSurface] = useState('dark')
  const [size, setSize] = useState(256)
  const dialogRef = useRef<HTMLDialogElement>(null)
  const selected = concepts.find((concept) => concept.id === selectedId) ?? concepts[0]

  return (
    <main className="kl">
      <header className="kl-header">
        <Link href="/" className="kl-brand" aria-label="Kuizer home">
          <Wordmark />
        </Link>
        <span className="kl-header-divider" />
        <span className="kl-eyebrow">Identity studies</span>
        <a className="kl-header-link" href="#context">
          See in context <span aria-hidden="true">↗</span>
        </a>
      </header>

      <div className="kl-content">
        <section className="kl-intro" aria-labelledby="lab-title">
          <div>
            <p className="kl-eyebrow">
              <span className="kl-dot" /> The icon collection / Vol. 01
            </p>
            <h1 id="lab-title">
              An identity for <span>kuizer.</span>
            </h1>
            <p className="kl-lead">
              Four directions. A little glass, a little metal, a lot of detail.
              <br />
              An exploration of the objects that could represent a cleaner workspace.
            </p>
          </div>
          <div className="kl-edition">
            <span>{String(concepts.length).padStart(2, '0')}</span>
            <p>
              Original concepts
              <br />
              Material & form studies
            </p>
          </div>
        </section>

        <section aria-label="Compare logo concepts">
          <div className="kl-toolbar">
            <p className="kl-eyebrow">
              01 <span className="kl-toolbar-slash">/</span> Explore the directions
            </p>
            <div className="kl-controls">
              <label className="kl-size">
                Icon size
                <select value={size} onChange={(event) => setSize(Number(event.target.value))}>
                  <option value={128}>128 px</option>
                  <option value={192}>192 px</option>
                  <option value={256}>256 px</option>
                </select>
              </label>
              <fieldset className="kl-segment" aria-label="Preview background">
                <button type="button" aria-pressed={surface === 'light'} onClick={() => setSurface('light')}>
                  <span aria-hidden="true">☼</span> Light
                </button>
                <button type="button" aria-pressed={surface === 'dark'} onClick={() => setSurface('dark')}>
                  <span aria-hidden="true">◐</span> Dark
                </button>
              </fieldset>
            </div>
          </div>

          <div className="kl-grid" data-surface={surface}>
            {concepts.map((concept) => (
              <article className="kl-concept" key={concept.id} data-selected={selectedId === concept.id}>
                <button
                  className="kl-stage"
                  type="button"
                  aria-label={`Select ${concept.name}`}
                  aria-pressed={selectedId === concept.id}
                  onClick={() => setSelectedId(concept.id)}
                >
                  <span className="kl-stage-number">{concept.number} /</span>
                  <Image
                    src={concept.image}
                    alt={concept.alt}
                    width={1254}
                    height={1254}
                    preload
                    style={{ width: size, height: size }}
                  />
                  <span className="kl-stage-note">
                    {selectedId === concept.id ? 'Selected for preview' : 'Select to preview'}
                    <span aria-hidden="true">↗</span>
                  </span>
                </button>
                <div className="kl-concept-heading">
                  <h2>{concept.name}</h2>
                  <span>{concept.category}</span>
                </div>
                <p className="kl-description">{concept.description}</p>
                <div className="kl-card-footer">
                  <div className="kl-swatches" aria-label={`${concept.name} palette`}>
                    {concept.palette.map((color) => (
                      <span key={color} style={{ backgroundColor: color }} title={color} />
                    ))}
                  </div>
                  <a
                    href={concept.image}
                    download={`kuizer-${concept.id}.png`}
                    aria-label={`Download ${concept.name} PNG`}
                  >
                    Download PNG <span aria-hidden="true">↓</span>
                  </a>
                </div>
              </article>
            ))}
          </div>
        </section>

        <section className="kl-context" id="context" aria-labelledby="context-title">
          <div className="kl-context-top">
            <p className="kl-eyebrow">
              02 <span className="kl-toolbar-slash">/</span> Off the artboard
            </p>
            <span className="kl-context-current">Previewing {selected.name}</span>
          </div>
          <div className="kl-context-grid">
            <div className="kl-wallpaper">
              <div className="kl-window">
                <div className="kl-window-bar">
                  <span />
                  <span />
                  <span />
                  <p>kuizer</p>
                </div>
                <div className="kl-app-heading">
                  <Image src={selected.image} width={64} height={64} alt="" />
                  <div>
                    <h3>A little more breathing room.</h3>
                    <p>Your workspace, tidied up.</p>
                  </div>
                </div>
                <div className="kl-disk-bar">
                  <span />
                  <span />
                  <span />
                </div>
                <div className="kl-disk-caption">
                  <span>Development files</span>
                  <span>12.8 GB reclaimable</span>
                </div>
              </div>
              <div className="kl-dock" aria-label="Illustrative desktop dock">
                <div className="kl-dock-folder" aria-hidden="true" />
                <div className="kl-dock-terminal" aria-hidden="true">
                  ›_
                </div>
                <span className="kl-dock-divider" />
                <div className="kl-dock-selected">
                  <Image
                    src={selected.image}
                    alt={`${selected.name} in the desktop dock`}
                    width={72}
                    height={72}
                  />
                  <span />
                </div>
                <div className="kl-dock-notes" aria-hidden="true">
                  <i />
                  <i />
                  <i />
                </div>
              </div>
              <span className="kl-wallpaper-caption">Desktop study · illustrative interface</span>
            </div>
            <div className="kl-detail" aria-live="polite" aria-atomic="true">
              <span className="kl-eyebrow">Direction {selected.number}</span>
              <h2 id="context-title">
                {selected.name}
                <span>.</span>
              </h2>
              <p>{selected.rationale}</p>
              <dl>
                <dt>Material language</dt>
                <dd>{selected.material}</dd>
              </dl>
              <div className="kl-scale" aria-label="Icon size comparison">
                {[24, 32, 48, 64].map((iconSize) => (
                  <div key={iconSize}>
                    <Image
                      src={selected.image}
                      alt={`${selected.name} at ${iconSize} pixels`}
                      width={iconSize}
                      height={iconSize}
                    />
                    <span>{iconSize}</span>
                  </div>
                ))}
              </div>
              <button type="button" className="kl-zoom" onClick={() => dialogRef.current?.showModal()}>
                Inspect the details <span aria-hidden="true">↗</span>
              </button>
            </div>
          </div>
        </section>
        <footer className="kl-footer">
          <p>
            <strong>kuizer.</strong> A cleaner workspace starts here.
          </p>
          <span>Concept artwork · Built-in Image Gen · October 2026</span>
        </footer>
      </div>

      <dialog ref={dialogRef} className="kl-dialog" aria-labelledby="detail-title">
        <div className="kl-dialog-bar">
          <h2 id="detail-title">
            {selected.name} <span>/ Material study</span>
          </h2>
          <button type="button" onClick={() => dialogRef.current?.close()} aria-label="Close detail view">
            ×
          </button>
        </div>
        <div className="kl-dialog-image" data-surface={surface}>
          <Image src={selected.image} alt={selected.alt} width={1254} height={1254} />
        </div>
        <div className="kl-dialog-footer">
          <span>Original transparent PNG · 1254 × 1254</span>
          <a href={selected.image} download={`kuizer-${selected.id}.png`}>
            Download original ↓
          </a>
        </div>
      </dialog>
    </main>
  )
}
