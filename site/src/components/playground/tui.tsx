'use client'

import type { Category, DemoItem } from '@/content/demo'
import { formatBytes, formatCount, GiB, MiB } from '@/shared/helpers/bytes'
import type { Action } from './reducer'
import {
  buildRows,
  bytesOf,
  foundIds,
  foundTotal,
  freeAfter,
  hasFilters,
  itemById,
  itemState,
  projectChanges,
  selectedIds,
  visibleItems,
  type DeleteMode,
  type SortKey,
  type State,
  type ViewRow,
} from './model'
import { plural, uncommitted } from './text'
import {
  categoryTips,
  columnTips,
  confirmTips,
  footerTips,
  groupTip,
  headerTips,
  itemTips,
  safetyTip,
} from './tips'

type Dispatch = (action: Action) => void

export type Flash = {
  key: string
  at: number
}

type Props = {
  state: State
  dispatch: Dispatch
  flash: Flash | null
}

type KeysProps = Props

type StateProps = {
  state: State
}

const categoryClass: Record<Category, string> = {
  dependencies: 'text-t-orange',
  build: 'text-t-periwinkle',
  cache: 'text-t-teal',
  temp: 'text-muted',
}

const filterChip = 'animate-pop-icon rounded-[3px] px-1.5 inset-ring inset-ring-current'

const barWidth = 8

function sizeClass(bytes: number): string {
  if (bytes >= GiB) return 'text-t-red'
  if (bytes >= 100 * MiB) return 'text-t-yellow'
  return ''
}

function sizeBar(bytes: number, largest: number): string {
  const filled = largest === 0 ? 0 : Math.min(Math.ceil((bytes * barWidth) / largest), barWidth)
  return '█'.repeat(filled) + '░'.repeat(barWidth - filled)
}

export function Tui({ state, dispatch, flash }: Props) {
  return (
    <div className="screen-in relative flex min-h-0 flex-1 flex-col">
      <Header state={state} dispatch={dispatch} />
      <Table state={state} dispatch={dispatch} />
      <Detail state={state} />
      <Footer state={state} dispatch={dispatch} flash={flash} />
      {state.overlay.kind === 'confirm' && (
        <Confirm
          state={state}
          ids={state.overlay.ids}
          mode={state.overlay.mode}
          dispatch={dispatch}
          flash={flash}
        />
      )}
      {state.overlay.kind === 'help' && <Help dispatch={dispatch} />}
    </div>
  )
}

type HeaderButtonProps = {
  keyName: string
  tip: string
  className: string
  dispatch: Dispatch
  children: React.ReactNode
}

function HeaderButton({ keyName, tip, className, dispatch, children }: HeaderButtonProps) {
  return (
    <button
      type="button"
      tabIndex={-1}
      data-tip={tip}
      onClick={() => dispatch({ type: 'key', key: keyName })}
      className={`${className} transition-colors duration-150 pointer-fine:hover:text-fg active:scale-96`}
    >
      {children}
    </button>
  )
}

function Header({ state, dispatch }: Omit<Props, 'flash'>) {
  const status = state.scan.finished ? `done in ${state.scan.elapsed.toFixed(1)}s` : 'scanning'
  const statusKey = state.scan.finished ? 'done' : 'scanning'
  const selected = bytesOf(selectedIds(state))
  const note =
    selected === 0
      ? null
      : state.mode === 'permanent'
        ? `→ ${formatBytes(freeAfter(state, selected, 'permanent'))} after delete`
        : 'selection stays on disk in the trash'
  const tips = headerTips(state)
  return (
    <div className="flex shrink-0 items-center gap-x-3 overflow-hidden whitespace-nowrap px-3 py-1.5">
      <span className="font-semibold" data-tip={tips.root}>
        kuizer
      </span>
      <span className="text-muted" data-tip={tips.status}>
        <span key={statusKey} className="animate-pop-icon inline-block">
          {status}
        </span>
        <span className="hidden sm:inline">
          {' '}
          {formatCount(state.scan.dirs)} dirs {formatCount(state.scan.files)} files 0 issues
        </span>
      </span>
      <span data-tip={tips.found}>
        {foundIds(state).length} found {formatBytes(foundTotal(state))}
      </span>
      <HeaderButton keyName="s" tip={tips.sort} className="text-muted" dispatch={dispatch}>
        sort {state.sort}
      </HeaderButton>
      {state.grouping === 'project' && (
        <HeaderButton keyName="p" tip={tips.grouping} className="text-muted" dispatch={dispatch}>
          by project
        </HeaderButton>
      )}
      {state.category !== null && (
        <HeaderButton
          keyName="c"
          tip={tips.category}
          className={`${filterChip} ${categoryClass[state.category]}`}
          dispatch={dispatch}
        >
          {state.category}
        </HeaderButton>
      )}
      {state.safeOnly && (
        <HeaderButton
          keyName="f"
          tip={tips.safeOnly}
          className={`${filterChip} text-t-green`}
          dispatch={dispatch}
        >
          safe only
        </HeaderButton>
      )}
      {state.filter !== '' && (
        <HeaderButton keyName="/" tip={tips.filter} className={filterChip} dispatch={dispatch}>
          &quot;{state.filter}&quot;
        </HeaderButton>
      )}
      <span className="ml-auto hidden sm:inline" data-tip={tips.free}>
        {formatBytes(state.free)} free
      </span>
      {note && (
        <span className="text-muted hidden md:inline" data-tip={tips.note}>
          {note}
        </span>
      )}
    </div>
  )
}

function Table({ state, dispatch }: Omit<Props, 'flash'>) {
  const rows = buildRows(state)
  const items = visibleItems(state)
  const largest = items.reduce((max, item) => Math.max(max, item.bytes), 0)
  return (
    <div className="relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-3" data-table>
      <span className="tui-cursor" aria-hidden="true" />
      <div className="tui-row text-muted sticky top-0 z-10 bg-raised">
        <span data-tip={columnTips.mark} />
        <ColumnButton
          sortKey="size"
          tip={columnTips.size}
          state={state}
          dispatch={dispatch}
          className="text-right"
        >
          SIZE
        </ColumnButton>
        <span className="hidden md:block" data-tip={columnTips.bar} />
        <span data-tip={columnTips.safe}>SAFE?</span>
        <ColumnButton
          sortKey="category"
          tip={columnTips.category}
          state={state}
          dispatch={dispatch}
          className="hidden md:block"
        >
          CATEGORY
        </ColumnButton>
        <span className="hidden md:block" data-tip={columnTips.rule}>
          RULE
        </span>
        <span className="hidden md:block" data-tip={columnTips.age}>
          AGE
        </span>
        <ColumnButton sortKey="path" tip={columnTips.path} state={state} dispatch={dispatch}>
          PATH
        </ColumnButton>
      </div>
      {rows.length === 0 && (
        <div className="text-muted py-2">
          {state.scan.finished
            ? hasFilters(state)
              ? 'Nothing matches these filters. x clears them.'
              : 'Nothing found.'
            : 'Scanning, results appear as they are found.'}
        </div>
      )}
      {rows.map((row) => (
        <Row
          key={row.kind === 'group' ? `group:${row.key}` : row.item.id}
          row={row}
          state={state}
          largest={largest}
          dispatch={dispatch}
        />
      ))}
    </div>
  )
}

type ColumnButtonProps = {
  sortKey: SortKey
  tip: string
  state: State
  dispatch: Dispatch
  className?: string
  children: React.ReactNode
}

function ColumnButton({ sortKey, tip, state, dispatch, className = '', children }: ColumnButtonProps) {
  const active = state.sort === sortKey
  return (
    <button
      type="button"
      tabIndex={-1}
      data-tip={tip}
      aria-label={`Sort by ${sortKey}`}
      onClick={() => dispatch({ type: 'sort', key: sortKey })}
      className={`${className} ${active ? 'text-fg' : ''} transition-colors duration-150 pointer-fine:hover:text-fg`}
    >
      {children}
    </button>
  )
}

type RowProps = {
  row: ViewRow
  state: State
  largest: number
  dispatch: Dispatch
}

function Row({ row, state, largest, dispatch }: RowProps) {
  if (row.kind === 'group') {
    return (
      <button
        type="button"
        tabIndex={-1}
        data-tip={groupTip(row)}
        onClick={() => dispatch({ type: 'selectGroup', key: row.key })}
        className={`tui-row w-full text-left text-muted font-semibold transition-colors pointer-fine:hover:text-fg ${state.scan.finished ? '' : 'rise'}`}
      >
        <span className="col-span-full">
          {row.title}
          {`  · ${plural(row.count, 'item')}  · ${formatBytes(row.bytes)}`}
          {row.changes > 0 && <span className="text-t-yellow">{`  · ${uncommitted(row.changes)}`}</span>}
        </span>
      </button>
    )
  }
  const { item, position } = row
  const entry = itemState(state, item.id)
  const ready = entry.status === 'ready'
  const mark =
    entry.status === 'deleted' ? '  ✓' : entry.status === 'deleting' ? '  …' : entry.selected ? '[x]' : '[ ]'
  const tone = ready
    ? ''
    : entry.status === 'deleted'
      ? 'relative text-faint row-deleted'
      : 'text-muted opacity-45'
  const highlight = entry.selected && ready ? 'bg-(--t-selected)' : ''
  const tips = itemTips(state, item, entry, largest)
  function dim(name: string) {
    return ready ? name : 'text-muted'
  }
  return (
    <button
      type="button"
      data-position={position}
      aria-pressed={entry.selected}
      aria-label={`${item.path}, ${formatBytes(item.bytes)}, ${item.safety}${entry.selected ? ', selected' : ''}`}
      onClick={() => dispatch({ type: 'row', position })}
      onPointerMove={(event) => {
        const moved = event.movementX !== 0 || event.movementY !== 0
        if (event.pointerType === 'mouse' && moved) dispatch({ type: 'hover', position })
      }}
      className={`tui-row w-full text-left ${state.scan.finished ? '' : 'rise'} ${tone} ${highlight}`}
    >
      <span className="text-muted whitespace-pre" data-tip={tips.mark}>
        <span key={mark} className={`inline-block ${ready ? '' : 'animate-pop-icon'}`}>
          {mark}
        </span>
      </span>
      <span className={`text-right ${dim(sizeClass(item.bytes))}`} data-tip={tips.size}>
        {formatBytes(item.bytes)}
      </span>
      <span className="hidden md:block text-faint" data-tip={tips.bar}>
        {sizeBar(item.bytes, largest)}
      </span>
      <span className={dim(item.safety === 'safe' ? 'text-t-green' : 'text-t-yellow')} data-tip={tips.safety}>
        {item.safety === 'safe' ? '● safe' : '▲ check'}
      </span>
      <span className={`hidden md:block ${dim(categoryClass[item.category])}`} data-tip={tips.category}>
        {item.category}
      </span>
      <span className="hidden md:block" data-tip={tips.rule}>
        {item.rule}
      </span>
      <span className="hidden md:block text-muted" data-tip={tips.age}>
        {item.age}
      </span>
      <span className="clip-left" data-tip={tips.path}>
        <span>{item.path}</span>
      </span>
    </button>
  )
}

function Detail({ state }: StateProps) {
  const item = visibleItems(state)[state.cursor]
  if (!item) {
    return (
      <div className="text-muted shrink-0 border-t border-line px-3 py-1.5 leading-normal">
        {state.scan.finished ? 'Nothing found.' : 'Scanning, results appear as they are found.'}
        <br />
        <br />
      </div>
    )
  }
  const changes = projectChanges(item.project)
  return (
    <div className="shrink-0 overflow-hidden whitespace-nowrap border-t border-line px-3 py-1.5 leading-normal">
      <div className="truncate" data-tip={item.path}>
        {item.path}
      </div>
      <div
        className={`truncate ${item.safety === 'safe' ? 'text-t-green' : 'text-t-yellow'}`}
        data-tip={categoryTips[item.category]}
      >
        {safetyTip(item.category, item.safety)}
      </div>
      <div className="text-muted truncate">
        <span data-tip={columnTips.rule}>{item.rule}</span>
        {` · ${item.ecosystem} · `}
        <span data-tip="Files and directories inside, counted during the scan.">
          {formatCount(item.entries)} entries
        </span>
        {` · project ${item.project}`}
        {changes > 0 && (
          <span
            className="text-t-yellow"
            data-tip="Git reports unsaved work in this project. Look before deleting."
          >
            {` · ${uncommitted(changes)}`}
          </span>
        )}
      </div>
    </div>
  )
}

type KeyProps = {
  keyName: string
  label: string
  tip: string
  dispatch: Dispatch
  flash: Flash | null
}

function Key({ keyName, label, tip, dispatch, flash }: KeyProps) {
  const flashed = flash !== null && flash.key === keyName
  return (
    <button
      type="button"
      data-tip={tip}
      onClick={() => dispatch({ type: 'key', key: keyName })}
      className="group text-muted inline-flex items-center gap-1.5 rounded-sm transition-colors duration-150 hover:text-fg focus-visible:text-fg focus-visible:transition-none"
    >
      <kbd
        key={flashed ? flash.at : 'idle'}
        className={`key-chip group-active:scale-96 pointer-fine:group-hover:bg-fg pointer-fine:group-hover:text-bg group-focus-visible:bg-fg group-focus-visible:text-bg group-focus-visible:transition-none ${flashed ? 'animate-key-flash' : ''}`}
      >
        {keyName === ' ' ? 'space' : keyName === 'Enter' ? 'enter' : keyName}
      </kbd>
      <span>{label}</span>
    </button>
  )
}

function Footer({ state, dispatch, flash }: KeysProps) {
  const selected = selectedIds(state)
  const tips = footerTips(state)
  if (state.filtering) {
    return (
      <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-t border-line px-3 py-1.5">
        <span
          className="font-semibold"
          data-tip="Keys type into the filter now. Motions are off until you leave it."
        >
          filter
        </span>
        <span className="caret">{state.filter}</span>
        <Key keyName="Enter" label="apply" tip={tips.apply} dispatch={dispatch} flash={flash} />
        <Key keyName="Escape" label="clear" tip={tips.cancel} dispatch={dispatch} flash={flash} />
      </div>
    )
  }
  return (
    <div className="flex shrink-0 flex-wrap items-center gap-x-3.5 gap-y-1 border-t border-line px-3 py-1.5">
      <Key keyName=" " label="select" tip={tips.select} dispatch={dispatch} flash={flash} />
      <Key keyName="a" label="select all safe" tip={tips.selectSafe} dispatch={dispatch} flash={flash} />
      {selected.length > 0 ? (
        <Key
          keyName="Enter"
          label="delete selected"
          tip={tips.deleteSelected}
          dispatch={dispatch}
          flash={flash}
        />
      ) : (
        <Key keyName="d" label="delete this" tip={tips.deleteThis} dispatch={dispatch} flash={flash} />
      )}
      <Key keyName="f" label="safe only" tip={tips.safeOnly} dispatch={dispatch} flash={flash} />
      <Key keyName="c" label="category" tip={tips.category} dispatch={dispatch} flash={flash} />
      <Key keyName="p" label="by project" tip={tips.project} dispatch={dispatch} flash={flash} />
      <Key keyName="/" label="search" tip={tips.search} dispatch={dispatch} flash={flash} />
      <Key keyName="?" label="help" tip={tips.help} dispatch={dispatch} flash={flash} />
      <Key keyName="q" label="quit" tip={tips.quit} dispatch={dispatch} flash={flash} />
      {hasFilters(state) && (
        <Key keyName="x" label="clear filters" tip={tips.clear} dispatch={dispatch} flash={flash} />
      )}
      {selected.length > 0 && (
        <Key keyName="u" label="clear" tip={tips.unselect} dispatch={dispatch} flash={flash} />
      )}
      {selected.length > 0 && (
        <button
          type="button"
          data-tip={tips.selected}
          onClick={() => dispatch({ type: 'key', key: 'Enter' })}
          className="rise font-semibold transition-colors pointer-fine:hover:text-fg active:scale-96"
        >
          {selected.length} selected {formatBytes(bytesOf(selected))}
        </button>
      )}
      {state.freed > 0 && <span data-tip={tips.freed}>freed {formatBytes(state.freed)}</span>}
      {state.trashed > 0 && <span data-tip={tips.trashed}>trashed {formatBytes(state.trashed)}</span>}
      {state.message && (
        <span key={state.message} className="text-muted rise">
          {state.message}
        </span>
      )}
    </div>
  )
}

type OverlayProps = {
  title: string
  onDismiss: () => void
  children: React.ReactNode
  rounded?: boolean
}

function Overlay({ title, onDismiss, children, rounded = false }: OverlayProps) {
  return (
    <>
      <div
        className="absolute inset-0 z-15 animate-fade bg-bg/55 backdrop-blur-[1.5px]"
        aria-hidden="true"
        onClick={onDismiss}
      />
      <div className="absolute inset-0 z-20 flex items-center justify-center p-3">
        <dialog
          open
          aria-label={title}
          className={`relative m-0 animate-pop border border-muted shadow-[0_24px_60px_rgb(0_0_0/0.6)] flex max-h-full w-full max-w-overlay flex-col bg-raised text-inherit ${rounded ? 'rounded-lg' : ''}`}
        >
          <span className="absolute -top-[0.8em] left-3 bg-raised px-1 font-semibold">{title}</span>
          <div className="min-h-0 overflow-auto px-3 py-3 leading-copy">{children}</div>
        </dialog>
      </div>
    </>
  )
}

type ConfirmProps = {
  state: State
  ids: string[]
  mode: DeleteMode
  dispatch: Dispatch
  flash: Flash | null
}

const shown = 8
const dirtyShown = 3

function Confirm({ state, ids, mode, dispatch, flash }: ConfirmProps) {
  const items = ids.map(itemById)
  const bytes = bytesOf(ids)
  const review = items.filter((item) => item.safety === 'review').length
  const dirty: DemoItem['project'][] = []
  for (const item of items) {
    if (projectChanges(item.project) > 0 && !dirty.includes(item.project)) dirty.push(item.project)
  }
  const action =
    mode === 'trash'
      ? 'Move to trash. Space is freed once the trash is emptied.'
      : 'Delete permanently. This cannot be undone.'
  const tips = confirmTips(mode)
  return (
    <Overlay
      title={`Delete ${plural(items.length, 'item')} · ${formatBytes(bytes)}`}
      onDismiss={() => dispatch({ type: 'key', key: 'n' })}
    >
      <div className="mt-1">
        {items.slice(0, shown).map((item) => (
          <div key={item.id} className="flex gap-3 whitespace-nowrap">
            <span className="w-amount shrink-0 text-right">{formatBytes(item.bytes)}</span>
            <span className="clip-left" data-tip={`${item.path}\n${safetyTip(item.category, item.safety)}`}>
              <span>{item.path}</span>
            </span>
          </div>
        ))}
        {items.length > shown && <div className="text-muted">and {items.length - shown} more</div>}
      </div>
      <div className="mt-3 font-semibold">{action}</div>
      <div>
        {formatBytes(state.free)} free now, {formatBytes(freeAfter(state, bytes, mode))} free afterwards.
      </div>
      {review > 0 && (
        <div>{review} of these are marked review: check they are generated before deleting.</div>
      )}
      {dirty.slice(0, dirtyShown).map((project) => (
        <div key={project} className="text-t-yellow">
          {project} has {uncommitted(projectChanges(project))}.
        </div>
      ))}
      {dirty.length > dirtyShown && (
        <div className="text-t-yellow">
          and {dirty.length - dirtyShown} more projects with uncommitted changes.
        </div>
      )}
      <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1">
        <Key keyName="y" label="confirm" tip={tips.confirm} dispatch={dispatch} flash={flash} />
        <Key
          keyName="t"
          label={mode === 'trash' ? 'switch to permanent' : 'switch to trash'}
          tip={tips.toggle}
          dispatch={dispatch}
          flash={flash}
        />
        <Key keyName="n" label="cancel" tip={tips.cancel} dispatch={dispatch} flash={flash} />
      </div>
    </Overlay>
  )
}

type HelpRow = { keys: string[]; action: string }
type HelpSection = { title: string; rows: HelpRow[] }

const helpSections: HelpSection[] = [
  {
    title: 'Navigate',
    rows: [
      { keys: ['j', 'k', '↑', '↓'], action: 'move' },
      { keys: ['g', 'home'], action: 'first' },
      { keys: ['G', 'end'], action: 'last' },
      { keys: ['pgup', 'pgdn'], action: 'page' },
    ],
  },
  {
    title: 'Select',
    rows: [
      { keys: ['space'], action: 'toggle and move down' },
      { keys: ['a'], action: 'select every safe item in view' },
      { keys: ['P'], action: "select every item in the cursor's project" },
      { keys: ['u'], action: 'clear selection' },
    ],
  },
  {
    title: 'Filter',
    rows: [
      { keys: ['f'], action: 'show only items that are safe to delete' },
      { keys: ['c'], action: 'cycle category: dependencies, build, cache, temp' },
      { keys: ['/'], action: 'search by path or rule' },
      { keys: ['x'], action: 'clear all filters' },
    ],
  },
  {
    title: 'Act',
    rows: [
      { keys: ['d', 'del'], action: 'delete selection, or the item under the cursor' },
      { keys: ['enter'], action: 'delete selection' },
      { keys: ['s'], action: 'cycle sort: size, path, category' },
      { keys: ['p'], action: 'group by rule or by project' },
      { keys: ['q', 'esc'], action: 'quit to the shell' },
    ],
  },
]

function Help({ dispatch }: { dispatch: Dispatch }) {
  return (
    <Overlay title="Keys" rounded onDismiss={() => dispatch({ type: 'key', key: 'Escape' })}>
      <button
        type="button"
        className="block w-full text-left focus-visible:text-fg"
        onClick={() => dispatch({ type: 'key', key: 'Escape' })}
      >
        <div className="grid gap-x-8 gap-y-3 sm:grid-cols-2">
          {helpSections.map((section) => (
            <div key={section.title}>
              <div className="text-muted font-semibold uppercase">{section.title}</div>
              {section.rows.map((row) => (
                <div key={row.action} className="flex items-baseline gap-2">
                  <span className="flex shrink-0 gap-1">
                    {row.keys.map((key) => (
                      <kbd key={key} className="key-chip">
                        {key}
                      </kbd>
                    ))}
                  </span>
                  <span>{row.action}</span>
                </div>
              ))}
            </div>
          ))}
        </div>
        <div className="text-faint mt-3">any key to close</div>
      </button>
    </Overlay>
  )
}
