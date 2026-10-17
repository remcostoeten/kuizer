import {
  categories,
  demoItems,
  diskFree,
  uncommittedChanges,
  type Category,
  type DemoItem,
} from '@/content/demo'

export type SortKey = 'size' | 'path' | 'category'
export type Grouping = 'rule' | 'project'
export type DeleteMode = 'trash' | 'permanent'
export type ItemStatus = 'ready' | 'deleting' | 'deleted'
export type Screen = 'boot' | 'shell' | 'tui'

export type Overlay =
  | { kind: 'none' }
  | { kind: 'help' }
  | { kind: 'confirm'; ids: string[]; mode: DeleteMode }

export type ItemState = {
  found: boolean
  selected: boolean
  status: ItemStatus
}

export type HistoryRun = {
  id: string
  at: string
  ids: string[]
  mode: DeleteMode
  restored: boolean
}

export type ShellTone = 'plain' | 'muted' | 'warn' | 'ok' | 'prompt'

export type ShellLine = {
  text: string
  tone: ShellTone
}

export type State = {
  screen: Screen
  typed: number
  shell: ShellLine[]
  shellHistory: string[]
  items: Record<string, ItemState>
  scan: { dirs: number; files: number; finished: boolean; elapsed: number }
  cursor: number
  sort: SortKey
  grouping: Grouping
  filter: string
  filtering: boolean
  category: Category | null
  safeOnly: boolean
  overlay: Overlay
  mode: DeleteMode
  freed: number
  trashed: number
  free: number
  message: string | null
  runs: HistoryRun[]
}

export type ViewRow =
  | { kind: 'group'; key: string; title: string; count: number; bytes: number; changes: number }
  | { kind: 'item'; item: DemoItem; position: number }

/**
 * @name itemById
 * @description Looks up a demo item by id. Ids only ever come from the demo
 * data, so a miss is a programming error.
 *
 * @example
 * const item = itemById('editor-target')
 */
export function itemById(id: string): DemoItem {
  const item = demoItems.find((entry) => entry.id === id)
  if (!item) throw new Error(`unknown demo item ${id}`)
  return item
}

/**
 * @name projectChanges
 * @description The number of uncommitted git changes the demo pretends a
 * project has, or 0 for a clean working tree.
 *
 * @example
 * projectChanges('~/projects/editor') // 3
 */
export function projectChanges(project: string): number {
  return uncommittedChanges[project] ?? 0
}

/**
 * @name initialState
 * @description The playground before anything happened: an empty shell about
 * to type the scan command, every artifact still on the fake disk.
 *
 * @example
 * const [state, dispatch] = useReducer(reduce, undefined, initialState)
 */
export function initialState(): State {
  const items: Record<string, ItemState> = {}
  for (const item of demoItems) {
    items[item.id] = { found: false, selected: false, status: 'ready' }
  }
  return {
    screen: 'boot',
    typed: 0,
    shell: [],
    shellHistory: [],
    items,
    scan: { dirs: 0, files: 0, finished: false, elapsed: 0 },
    cursor: 0,
    sort: 'size',
    grouping: 'rule',
    filter: '',
    filtering: false,
    category: null,
    safeOnly: false,
    overlay: { kind: 'none' },
    mode: 'trash',
    freed: 0,
    trashed: 0,
    free: diskFree,
    message: null,
    runs: [],
  }
}

/**
 * @name itemState
 * @description The playground's view of one item: whether the scan listed
 * it, whether it is ticked, and whether it is still on disk.
 *
 * @example
 * itemState(state, 'editor-target').selected
 */
export function itemState(state: State, id: string): ItemState {
  return state.items[id] ?? { found: false, selected: false, status: 'ready' }
}

/**
 * @name updateItems
 * @description Returns a state with the given change applied to every listed
 * item.
 *
 * @example
 * updateItems(state, ['editor-target'], { selected: true })
 */
export function updateItems(state: State, ids: string[], change: Partial<ItemState>): State {
  const items = { ...state.items }
  for (const id of ids) {
    items[id] = { ...itemState(state, id), ...change }
  }
  return { ...state, items }
}

function matches(state: State, item: DemoItem): boolean {
  if (state.category !== null && item.category !== state.category) return false
  if (state.safeOnly && item.safety !== 'safe') return false
  if (state.filter === '') return true
  const needle = state.filter.toLowerCase()
  return item.path.toLowerCase().includes(needle) || item.rule.includes(needle)
}

function groupKey(state: State, item: DemoItem): string {
  if (state.grouping === 'project') return item.project
  return `${categories.indexOf(item.category)}:${item.rule}`
}

function compareText(left: string, right: string): number {
  if (left < right) return -1
  if (left > right) return 1
  return 0
}

/**
 * @name visibleItems
 * @description The items in display order after the active filters, sort and
 * grouping. Mirrors the refresh step of the real list.
 *
 * @example
 * const current = visibleItems(state)[state.cursor]
 */
export function visibleItems(state: State): DemoItem[] {
  const items = demoItems.filter((item) => itemState(state, item.id).found && matches(state, item))
  const totals = new Map<string, number>()
  for (const item of items) {
    const key = groupKey(state, item)
    totals.set(key, (totals.get(key) ?? 0) + item.bytes)
  }
  return [...items].sort((left, right) => {
    const leftKey = groupKey(state, left)
    const rightKey = groupKey(state, right)
    const groupOrder =
      state.sort === 'size'
        ? (totals.get(rightKey) ?? 0) - (totals.get(leftKey) ?? 0) || compareText(leftKey, rightKey)
        : compareText(leftKey, rightKey)
    if (groupOrder !== 0) return groupOrder
    if (state.sort === 'path') return compareText(left.path, right.path)
    return right.bytes - left.bytes || compareText(left.path, right.path)
  })
}

/**
 * @name buildRows
 * @description Turns the visible items into table rows with a header row at
 * every change of group, the way the real list is drawn.
 *
 * @example
 * buildRows(state).map((row) => (row.kind === 'group' ? row.title : row.item.path))
 */
export function buildRows(state: State): ViewRow[] {
  const items = visibleItems(state)
  const groups = new Map<string, { count: number; bytes: number }>()
  for (const item of items) {
    const key = groupKey(state, item)
    const entry = groups.get(key) ?? { count: 0, bytes: 0 }
    groups.set(key, { count: entry.count + 1, bytes: entry.bytes + item.bytes })
  }
  const rows: ViewRow[] = []
  let previous: string | null = null
  items.forEach((item, position) => {
    const key = groupKey(state, item)
    if (key !== previous) {
      const entry = groups.get(key) ?? { count: 0, bytes: 0 }
      rows.push({
        kind: 'group',
        key,
        title: state.grouping === 'project' ? item.project : `${item.category} / ${item.rule}`,
        count: entry.count,
        bytes: entry.bytes,
        changes: state.grouping === 'project' ? projectChanges(item.project) : 0,
      })
      previous = key
    }
    rows.push({ kind: 'item', item, position })
  })
  return rows
}

/**
 * @name currentId
 * @description The id of the item under the cursor, or null when the list is
 * empty.
 *
 * @example
 * const id = currentId(state)
 */
export function currentId(state: State): string | null {
  return visibleItems(state)[state.cursor]?.id ?? null
}

/**
 * @name keepCursor
 * @description After the list order changed, puts the cursor back on the
 * item it was on, or clamps it when that item left the view.
 *
 * @example
 * keepCursor({ ...state, sort: 'path' }, currentId(state))
 */
export function keepCursor(state: State, keepId: string | null): State {
  const items = visibleItems(state)
  const position = keepId === null ? -1 : items.findIndex((item) => item.id === keepId)
  const cursor = position >= 0 ? position : Math.min(state.cursor, Math.max(items.length - 1, 0))
  return { ...state, cursor }
}

/**
 * @name selectedIds
 * @description Ids of the items that are ticked and still on disk.
 *
 * @example
 * const bytes = bytesOf(selectedIds(state))
 */
export function selectedIds(state: State): string[] {
  return demoItems
    .filter((item) => {
      const entry = itemState(state, item.id)
      return entry.selected && entry.status === 'ready'
    })
    .map((item) => item.id)
}

/**
 * @name bytesOf
 * @description Sums the sizes of the given item ids.
 *
 * @example
 * bytesOf(selectedIds(state))
 */
export function bytesOf(ids: string[]): number {
  return ids.reduce((sum, id) => sum + itemById(id).bytes, 0)
}

/**
 * @name foundIds
 * @description Ids of the items the current scan has listed, deleted ones
 * included, since the list keeps showing them struck through.
 *
 * @example
 * foundIds(state).length
 */
export function foundIds(state: State): string[] {
  return demoItems.filter((item) => itemState(state, item.id).found).map((item) => item.id)
}

/**
 * @name foundTotal
 * @description Total size of everything the scan listed that is not deleted
 * yet, as shown in the header.
 *
 * @example
 * formatBytes(foundTotal(state))
 */
export function foundTotal(state: State): number {
  return demoItems
    .filter((item) => {
      const entry = itemState(state, item.id)
      return entry.found && entry.status !== 'deleted'
    })
    .reduce((sum, item) => sum + item.bytes, 0)
}

/**
 * @name onDiskIds
 * @description Ids of the items still present on the fake disk, which is what
 * a fresh scan would find.
 *
 * @example
 * onDiskIds(state).length
 */
export function onDiskIds(state: State): string[] {
  return demoItems.filter((item) => itemState(state, item.id).status === 'ready').map((item) => item.id)
}

/**
 * @name hasFilters
 * @description Whether a category, safe-only or search filter is active.
 *
 * @example
 * if (hasFilters(state)) showClearHint()
 */
export function hasFilters(state: State): boolean {
  return state.category !== null || state.safeOnly || state.filter !== ''
}

/**
 * @name freeAfter
 * @description Free disk space after removing the given bytes in the given
 * mode. Moving to the trash frees nothing until the trash is emptied.
 *
 * @example
 * freeAfter(state, bytesOf(ids), 'permanent')
 */
export function freeAfter(state: State, bytes: number, mode: DeleteMode): number {
  return mode === 'permanent' ? state.free + bytes : state.free
}

/**
 * @name startScan
 * @description Switches to the list and resets the scan, so results stream
 * in again. Items already deleted are no longer on disk and stay hidden.
 *
 * @example
 * dispatch({ type: 'launch' }) // reduces to startScan(state)
 */
export function startScan(state: State): State {
  const next = updateItems(state, Object.keys(state.items), { found: false, selected: false })
  return {
    ...next,
    screen: 'tui',
    scan: { dirs: 0, files: 0, finished: false, elapsed: 0 },
    cursor: 0,
    overlay: { kind: 'none' },
    filtering: false,
    message: null,
  }
}
