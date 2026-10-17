import { categories, scanTotals } from '@/content/demo'
import {
  currentId,
  itemById,
  itemState,
  keepCursor,
  selectedIds,
  startScan,
  updateItems,
  visibleItems,
  type DeleteMode,
  type HistoryRun,
  type SortKey,
  type State,
} from './model'
import { runShell } from './shell'

export type Action =
  | { type: 'typed' }
  | { type: 'launch' }
  | { type: 'reveal'; id: string }
  | { type: 'progress'; dirs: number; files: number }
  | { type: 'finish'; elapsed: number }
  | { type: 'deleted'; id: string }
  | { type: 'key'; key: string }
  | { type: 'row'; position: number }
  | { type: 'focusItem'; id: string }
  | { type: 'shell'; line: string }

const sortOrder: SortKey[] = ['size', 'path', 'category']

function moveBy(state: State, delta: number): State {
  const count = visibleItems(state).length
  if (count === 0) return state
  return { ...state, cursor: Math.min(Math.max(state.cursor + delta, 0), count - 1) }
}

function moveTo(state: State, index: number): State {
  const count = visibleItems(state).length
  return { ...state, cursor: Math.min(index, Math.max(count - 1, 0)) }
}

function toggleCurrent(state: State): State {
  const id = currentId(state)
  if (id === null) return state
  const entry = itemState(state, id)
  const next = entry.status === 'ready' ? updateItems(state, [id], { selected: !entry.selected }) : state
  return moveBy(next, 1)
}

function selectSafe(state: State): State {
  const ids = visibleItems(state)
    .filter((item) => item.safety === 'safe' && itemState(state, item.id).status === 'ready')
    .map((item) => item.id)
  return updateItems(state, ids, { selected: true })
}

function selectProject(state: State): State {
  const id = currentId(state)
  if (id === null) return state
  const project = itemById(id).project
  const ids = visibleItems(state)
    .filter((item) => item.project === project && itemState(state, item.id).status === 'ready')
    .map((item) => item.id)
  return updateItems(state, ids, { selected: true })
}

function clearSelection(state: State): State {
  return updateItems(state, Object.keys(state.items), { selected: false })
}

function requestDelete(state: State): State {
  const selected = selectedIds(state)
  if (selected.length > 0) return { ...state, overlay: { kind: 'confirm', ids: selected, mode: state.mode } }
  const id = currentId(state)
  if (id === null || itemState(state, id).status !== 'ready') return state
  return { ...state, overlay: { kind: 'confirm', ids: [id], mode: state.mode } }
}

function confirmDelete(state: State, ids: string[], mode: DeleteMode): State {
  const ready = ids.filter((id) => itemState(state, id).status === 'ready')
  if (ready.length === 0) return { ...state, overlay: { kind: 'none' } }
  const run: HistoryRun = {
    id: `s${state.runs.length + 1}`,
    at: new Date().toISOString(),
    ids: ready,
    mode,
    restored: false,
  }
  const next = updateItems(state, ready, { status: 'deleting', selected: false })
  return { ...next, overlay: { kind: 'none' }, runs: [...state.runs, run], message: null }
}

function markDeleted(state: State, id: string): State {
  const run = [...state.runs].reverse().find((entry) => entry.ids.includes(id))
  if (!run || itemState(state, id).status !== 'deleting') return state
  const bytes = itemById(id).bytes
  const next = updateItems(state, [id], { status: 'deleted' })
  const permanent = run.mode === 'permanent'
  const remaining = run.ids.filter((other) => itemState(next, other).status === 'deleting').length
  const done = permanent
    ? `${run.ids.length} deleted permanently, logged as ${run.id}`
    : `${run.ids.length} moved to the trash, kuizer restore ${run.id} puts them back`
  return {
    ...next,
    freed: permanent ? state.freed + bytes : state.freed,
    trashed: permanent ? state.trashed : state.trashed + bytes,
    free: permanent ? state.free + bytes : state.free,
    message: remaining === 0 ? done : state.message,
  }
}

function withViewChange(state: State, change: Partial<State>): State {
  return keepCursor({ ...state, ...change }, currentId(state))
}

function handleFilterKey(state: State, key: string): State {
  if (key === 'Enter') return { ...state, filtering: false }
  if (key === 'Escape') return withViewChange(state, { filter: '', filtering: false })
  if (key === 'Backspace') return withViewChange(state, { filter: state.filter.slice(0, -1) })
  if (key.length === 1) return withViewChange(state, { filter: state.filter + key })
  return state
}

function cycleCategory(state: State): State {
  const index = state.category === null ? -1 : categories.indexOf(state.category)
  return withViewChange(state, { category: categories[index + 1] ?? null })
}

function cycleSort(state: State): State {
  const index = sortOrder.indexOf(state.sort)
  return withViewChange(state, { sort: sortOrder[(index + 1) % sortOrder.length] ?? 'size' })
}

function quit(state: State): State {
  return {
    ...state,
    screen: 'shell',
    overlay: { kind: 'none' },
    filtering: false,
    shell: [...state.shell, { text: '', tone: 'plain' }],
  }
}

function handleKey(state: State, key: string): State {
  const cleared = { ...state, message: null }
  if (state.filtering) return handleFilterKey(cleared, key)
  if (state.overlay.kind === 'confirm') {
    if (key === 'y' || key === 'Enter') return confirmDelete(cleared, state.overlay.ids, state.overlay.mode)
    if (key === 't') {
      const mode: DeleteMode = state.overlay.mode === 'trash' ? 'permanent' : 'trash'
      return { ...cleared, mode, overlay: { ...state.overlay, mode } }
    }
    return { ...cleared, overlay: { kind: 'none' } }
  }
  if (state.overlay.kind === 'help') return { ...cleared, overlay: { kind: 'none' } }
  switch (key) {
    case 'q':
    case 'Escape':
      return quit(cleared)
    case 'j':
    case 'ArrowDown':
      return moveBy(cleared, 1)
    case 'k':
    case 'ArrowUp':
      return moveBy(cleared, -1)
    case 'PageDown':
      return moveBy(cleared, 10)
    case 'PageUp':
      return moveBy(cleared, -10)
    case 'g':
    case 'Home':
      return moveTo(cleared, 0)
    case 'G':
    case 'End':
      return moveTo(cleared, Number.MAX_SAFE_INTEGER)
    case ' ':
      return toggleCurrent(cleared)
    case 'a':
      return selectSafe(cleared)
    case 'u':
      return clearSelection(cleared)
    case 'd':
    case 'Delete':
      return requestDelete(cleared)
    case 'Enter':
      return selectedIds(cleared).length > 0 ? requestDelete(cleared) : cleared
    case 's':
      return cycleSort(cleared)
    case 'p':
      return withViewChange(cleared, { grouping: state.grouping === 'rule' ? 'project' : 'rule' })
    case 'P':
      return selectProject(cleared)
    case '/':
      return { ...cleared, filtering: true }
    case 'c':
      return cycleCategory(cleared)
    case 'f':
      return withViewChange(cleared, { safeOnly: !state.safeOnly })
    case 'x':
      return withViewChange(cleared, { category: null, safeOnly: false, filter: '' })
    case '?':
      return { ...cleared, overlay: { kind: 'help' } }
    default:
      return cleared
  }
}

function clickRow(state: State, position: number): State {
  const item = visibleItems(state)[position]
  if (!item) return state
  if (state.cursor !== position) return { ...state, cursor: position, message: null }
  const entry = itemState(state, item.id)
  if (entry.status !== 'ready') return state
  return updateItems(state, [item.id], { selected: !entry.selected })
}

/**
 * @name reduce
 * @description The playground reducer. Key actions follow the key map of the
 * real `kuizer` list; the others drive the scan, the deletions and the shell.
 *
 * @example
 * const [state, dispatch] = useReducer(reduce, undefined, initialState)
 * dispatch({ type: 'key', key: ' ' })
 */
export function reduce(state: State, action: Action): State {
  switch (action.type) {
    case 'typed':
      return { ...state, typed: state.typed + 1 }
    case 'launch':
      return startScan(state)
    case 'reveal': {
      if (itemState(state, action.id).status !== 'ready') return state
      const revealed = updateItems(state, [action.id], { found: true })
      return state.cursor === 0 ? revealed : keepCursor(revealed, currentId(state))
    }
    case 'progress':
      return { ...state, scan: { ...state.scan, dirs: action.dirs, files: action.files } }
    case 'finish':
      return {
        ...state,
        scan: { dirs: scanTotals.dirs, files: scanTotals.files, finished: true, elapsed: action.elapsed },
      }
    case 'deleted':
      return markDeleted(state, action.id)
    case 'key':
      return state.screen === 'tui' ? handleKey(state, action.key) : state
    case 'row':
      return clickRow(state, action.position)
    case 'focusItem': {
      const position = visibleItems(state).findIndex((item) => item.id === action.id)
      return position >= 0 ? { ...state, cursor: position, message: null } : state
    }
    case 'shell':
      return state.screen === 'shell' ? runShell(state, action.line) : state
  }
}
