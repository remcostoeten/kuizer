import {
  highlights,
  highlightsEyebrow,
  highlightsLead,
  highlightsTitle,
  type Highlight,
} from '@/content/highlights'

type Indexed = React.CSSProperties & { '--i': number }

type Tile = React.CSSProperties & {
  '--i': number
  '--x': string
  '--y': string
  '--w': string
  '--h': string
}

const finds = [
  { name: 'node_modules', manifest: 'package.json', size: '1.2 GiB' },
  { name: 'target', manifest: 'Cargo.toml', size: '3.4 GiB' },
  { name: '.next', manifest: 'next.config.ts', size: '310 MiB' },
  { name: '.venv', manifest: 'uv.lock', size: '820 MiB' },
] as const

const tiles = [
  { name: 'node_modules', size: '12.4 GiB', x: 0, y: 0, w: 56, h: 100 },
  { name: 'target', size: '7.1 GiB', x: 56, y: 0, w: 44, h: 56 },
  { name: '.next', size: '3.9 GiB', x: 56, y: 56, w: 26, h: 44 },
  { name: '', size: '', x: 82, y: 56, w: 18, h: 26 },
  { name: '', size: '', x: 82, y: 82, w: 18, h: 18 },
] as const

const runs = [
  { id: 's4', when: 'Oct 17, 20:31', freed: '27.5 GiB' },
  { id: 's3', when: 'Oct 15, 09:12', freed: '4.1 GiB' },
  { id: 's2', when: 'Oct 9, 18:44', freed: '11.8 GiB' },
] as const

const rows = [
  ['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p'],
  ['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'enter'],
  ['z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/'],
] as const

const sequence = ['j', 'k', 'space', 'enter', '/'] as const

function indexed(index: number): Indexed {
  return { '--i': index }
}

function placed(tile: (typeof tiles)[number], index: number): Tile {
  return {
    '--i': index,
    '--x': `${tile.x}%`,
    '--y': `${tile.y}%`,
    '--w': `${tile.w}%`,
    '--h': `${tile.h}%`,
  }
}

function FolderGlyph() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      aria-hidden="true"
    >
      <path d="M1.5 4.5v8h13v-7H7.5L6 4H1.5z" strokeLinejoin="round" />
    </svg>
  )
}

function PhotoGlyph() {
  return (
    <svg viewBox="0 0 96 64" className="photo-art" aria-hidden="true">
      <circle cx="70" cy="18" r="7" />
      <path d="M0 64 30 30l16 18 12-12 38 28z" />
    </svg>
  )
}

function Finds() {
  return (
    <div className="deck">
      {finds.map((item, index) => (
        <div key={item.name} className="deck-card" style={indexed(index)}>
          <span className="text-muted">
            <FolderGlyph />
          </span>
          <span className="flex min-w-0 flex-col leading-tight">
            <span className="text-fg">{item.name}</span>
            <span className="text-micro text-faint">beside {item.manifest}</span>
          </span>
          <span className="ms-auto flex flex-col items-end gap-1 leading-none">
            <span className="text-muted">{item.size}</span>
            <span className="pill-safe">safe</span>
          </span>
        </div>
      ))}
    </div>
  )
}

function Sizes() {
  return (
    <div className="map">
      {tiles.map((tile, index) => (
        <div
          key={index}
          className={index === 0 ? 'map-tile map-lead' : 'map-tile'}
          style={placed(tile, index)}
        >
          {tile.name && <span>{tile.name}</span>}
          {tile.size && <span className="text-faint">{tile.size}</span>}
        </div>
      ))}
      <span className="map-total">27.5 GiB</span>
    </div>
  )
}

function Duplicates() {
  return (
    <div className="dupes">
      <figure className="photo photo-keep">
        <PhotoGlyph />
        <figcaption>
          IMG_0412.jpg<span className="photo-tag text-fg">keep</span>
        </figcaption>
      </figure>
      <figure className="photo photo-twin">
        <PhotoGlyph />
        <figcaption>
          IMG_0412 (1).jpg<span className="photo-tag text-faint">duplicate</span>
        </figcaption>
      </figure>
      <span className="scan-line" />
      <span className="hash-chip">blake3 7f3a…c901</span>
    </div>
  )
}

function Trash() {
  return (
    <div className="dialog">
      <div className="dialog-ask">
        <span className="font-sans text-label font-medium text-fg">Move 14 items to the trash?</span>
        <span className="text-faint">27.5 GiB · restorable</span>
        <span className="mt-3 flex gap-2">
          <span className="dialog-button">
            <kbd>esc</kbd> Cancel
          </span>
          <span className="dialog-button dialog-primary">
            <kbd>enter</kbd> Move
          </span>
        </span>
      </div>
      <div className="dialog-done">
        <span className="dialog-check">
          <svg
            width="14"
            height="14"
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            aria-hidden="true"
          >
            <path d="m3.5 8.5 3 3 6-7" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </span>
        <span className="text-fg">14 items in the trash</span>
        <span className="text-faint">
          <kbd>u</kbd> to undo
        </span>
      </div>
    </div>
  )
}

function History() {
  return (
    <div className="runs">
      <div className="runs-track">
        {[...runs, runs[0]].map((run, index) => (
          <div key={index} className="run-card">
            <span className="flex items-center gap-2">
              <span className="run-id">{run.id}</span>
              <span className="text-faint">{run.when}</span>
            </span>
            <span className="text-section font-sans font-medium text-fg">{run.freed}</span>
            <span className="run-restore">kuizer restore {run.id}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

function Keys() {
  return (
    <div className="keys">
      <div className="pane">
        <span className="pane-cursor" />
        {finds.slice(0, 3).map((item, index) => (
          <span key={item.name} className="pane-row">
            <span className={index === 0 ? 'pane-box pane-box-live' : 'pane-box'} />
            <span className="text-fg">{item.name}</span>
            <span className="ms-auto text-faint">{item.size}</span>
          </span>
        ))}
        <span className="pane-status">
          <span className="pane-moved">1 item moved to the trash</span>
          <span className="pane-filter">
            <span className="text-brand-light">/</span>node
            <span className="pane-caret" />
          </span>
        </span>
      </div>
      <div className="board">
        {rows.map((row, rowIndex) => (
          <div key={rowIndex} className="board-row" style={indexed(rowIndex)}>
            {row.map((key) => (
              <Keycap key={key} label={key} />
            ))}
          </div>
        ))}
        <div className="board-row" style={indexed(3)}>
          <Keycap label="space" />
        </div>
      </div>
    </div>
  )
}

function Keycap({ label }: { label: string }) {
  const step = sequence.indexOf(label as (typeof sequence)[number])
  const wide = label.length > 1 ? ` keycap-${label}` : ''
  if (step < 0) return <kbd className={`keycap${wide}`}>{label}</kbd>
  return (
    <kbd className={`keycap keycap-live${wide}`} style={indexed(step)}>
      {label}
    </kbd>
  )
}

function Art({ kind }: { kind: Highlight['art'] }) {
  switch (kind) {
    case 'rules':
      return <Finds />
    case 'sizes':
      return <Sizes />
    case 'duplicates':
      return <Duplicates />
    case 'trash':
      return <Trash />
    case 'history':
      return <History />
    case 'keys':
      return <Keys />
  }
}

export function Highlights() {
  return (
    <section className="wrap pt-28 md:pt-36">
      <div className="field relative isolate">
        <i className="mark top-0 left-0" />
        <i className="mark top-0 left-full" />
        <i className="mark top-full left-0" />
        <i className="mark top-full left-full" />
        <div className="field-head">
          <span className="cell head-eyebrow flex items-center px-6 font-mono text-sm font-medium tracking-widest uppercase text-brand">
            {highlightsEyebrow}
          </span>
          <h2 className="cell head-title flex flex-col justify-center px-6 text-display font-medium md:text-display-lg">
            {highlightsTitle[0]}
            <span className="text-muted">{highlightsTitle[1]}</span>
          </h2>
          <p className="cell head-lead flex items-center px-6 text-feature text-muted md:text-body">
            {highlightsLead}
          </p>
        </div>
        <ul className="field-cards">
          {highlights.map((item) => (
            <li key={item.art} className="card">
              <div className="art font-mono text-ui" aria-hidden="true">
                <Art kind={item.art} />
              </div>
              <h3 className="cell col-span-5 row-span-2 flex flex-col items-center justify-center text-center text-section leading-tight font-medium">
                {item.title[0]}
                <span className="text-muted">{item.title[1]}</span>
              </h3>
              <p className="col-span-5 row-span-2 flex items-center justify-center px-6 text-center text-feature text-faint">
                {item.body}
              </p>
            </li>
          ))}
        </ul>
      </div>
    </section>
  )
}
