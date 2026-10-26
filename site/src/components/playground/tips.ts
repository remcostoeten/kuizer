import { demoRoot, type Category, type DemoItem, type Safety } from '@/content/demo'
import { formatBytes, formatCount } from '@/shared/helpers/bytes'
import {
  bytesOf,
  foundTotal,
  projectChanges,
  selectedIds,
  type DeleteMode,
  type ItemState,
  type State,
  type ViewRow,
} from './model'
import { plural, uncommitted } from './text'

export const columnTips = {
  mark: 'Selection. Space toggles the row under the cursor.',
  size: 'Size on disk, counted file by file. Click to sort by size.',
  bar: 'Size next to the largest item in view.',
  safe: 'safe: a tool regenerates it.\ncheck: it may hold work you want to keep.',
  category: 'dependencies, build output, tool cache or temp files. Click to sort by category.',
  rule: 'The detection rule that matched this directory.',
  age: 'Time since anything inside last changed.',
  path: 'Where it lives. Long paths are clipped on the left. Click to sort by path.',
}

export const categoryTips: Record<Category, string> = {
  dependencies: 'Installed packages. The package manager puts them back.',
  build: 'Build output. The next build recreates it.',
  cache: 'Tool cache. Rebuilt the next time the tool needs it.',
  temp: 'Temporary files nothing depends on.',
}

/**
 * @name safetyTip
 * @description Explains what deleting an item of this category and safety
 * level means, in the words the detail pane uses.
 *
 * @example
 * safetyTip('build', 'safe') // 'Build output. Deleting is safe, the next build recreates it.'
 */
export function safetyTip(category: Category, safety: Safety): string {
  if (safety === 'review') {
    return 'Check first: this may hold data that cannot be regenerated. Look inside before deleting.'
  }
  switch (category) {
    case 'dependencies':
      return 'Installed packages. Deleting is safe, reinstall with your package manager.'
    case 'build':
      return 'Build output. Deleting is safe, the next build recreates it.'
    case 'cache':
      return 'Cache. Deleting is safe, it is rebuilt when needed.'
    case 'temp':
      return 'Temporary files. Deleting is safe.'
  }
}

/**
 * @name headerTips
 * @description Tooltip copy for every chip in the status line, phrased for
 * the current scan, sort, grouping and filters.
 *
 * @example
 * headerTips(state).sort // 'Sorted by size. s cycles size, path, category.'
 */
export function headerTips(state: State) {
  const { dirs, files, finished, elapsed } = state.scan
  return {
    root: `The scan root. Everything below ${demoRoot} was walked.`,
    status: finished
      ? `Walked ${formatCount(dirs)} directories and ${formatCount(files)} files in ${elapsed.toFixed(1)}s.`
      : `Walking ${demoRoot}. Rows appear the moment a rule matches.`,
    found: `Everything that matched a rule. Removing all of it frees ${formatBytes(foundTotal(state))}.`,
    sort: `Sorted by ${state.sort}. Click or s cycles size, path, category.`,
    grouping: 'Rows are grouped by project. Click or p groups by rule again.',
    category: `Only ${state.category ?? 'this category'} is shown. click or c moves to the next category, x clears.`,
    safeOnly: 'Items marked check are hidden. Click, f or x brings them back.',
    filter: `Only paths and rules containing "${state.filter}". click or / edits it, x clears.`,
    free: `Free space on the disk that holds ${demoRoot}.`,
    note:
      state.mode === 'permanent'
        ? 'Permanent deletes free space the moment they finish.'
        : 'Trashed files still take space until the trash is emptied.',
  }
}

/**
 * @name itemTips
 * @description Tooltip copy for each cell of one result row, aware of whether
 * the item is selected, being deleted or already gone.
 *
 * @example
 * itemTips(state, item, itemState(state, item.id), largest).rule
 */
export function itemTips(state: State, item: DemoItem, entry: ItemState, largest: number) {
  const run = state.runs.find((candidate) => candidate.ids.includes(item.id))
  const share = largest === 0 ? 0 : Math.round((item.bytes / largest) * 100)
  const changes = projectChanges(item.project)
  const mark =
    entry.status === 'deleted'
      ? run?.mode === 'permanent'
        ? 'Deleted permanently.'
        : `In the trash. kuizer restore ${run?.id ?? ''} puts it back.`
      : entry.status === 'deleting'
        ? 'Deleting.'
        : entry.selected
          ? 'Selected. Space or a click deselects it.'
          : 'Not selected. Space or a click selects it.'
  return {
    mark,
    size: `${formatBytes(item.bytes)} across ${formatCount(item.entries)} entries.`,
    bar: `${share}% of the largest item in view.`,
    safety: safetyTip(item.category, item.safety),
    category: categoryTips[item.category],
    rule: `Matched the ${item.rule} rule for ${item.ecosystem} projects.`,
    age: `Last changed ${item.age} ago.`,
    path: `${item.path}\nproject ${item.project}${changes > 0 ? `, ${uncommitted(changes)}` : ''}`,
  }
}

/**
 * @name groupTip
 * @description Tooltip copy for a group heading: what it holds and whether
 * the project behind it has uncommitted work.
 *
 * @example
 * groupTip(row) // '3 items, 27.5 GiB in total.'
 */
export function groupTip(row: Extract<ViewRow, { kind: 'group' }>): string {
  const summary = `${plural(row.count, 'item')}, ${formatBytes(row.bytes)} in total. Click to select the group.`
  if (row.changes === 0) return summary
  return `${summary}\n${uncommitted(row.changes)} in this project. Look before deleting.`
}

/**
 * @name footerTips
 * @description Tooltip copy for the footer keys, phrased for what each key
 * does from the current state.
 *
 * @example
 * footerTips(state).safeOnly
 */
export function footerTips(state: State) {
  const selected = selectedIds(state)
  return {
    select: 'Toggle the item under the cursor, then move down one.',
    selectSafe: 'Select every item marked safe in the current view.',
    deleteSelected: `Delete ${plural(selected.length, 'selected item')}. A confirmation shows the count and size first.`,
    deleteThis: 'Delete the item under the cursor. A confirmation comes first.',
    safeOnly: state.safeOnly
      ? 'Show items marked check again.'
      : 'Hide items marked check, keep the safe ones.',
    category:
      state.category === null
        ? 'Show one category at a time: dependencies, build, cache, temp.'
        : `Next category after ${state.category}. Cycles back to all.`,
    project:
      state.grouping === 'rule'
        ? 'Group rows by project instead of by rule.'
        : 'Group rows by rule instead of by project.',
    search: 'Type to narrow by path or rule. Enter keeps it, esc drops it.',
    help: 'Every key on one screen.',
    quit: 'Back to the shell. Type kuizer there to come back.',
    unselect: 'Clear the selection. Nothing is deleted.',
    clear: 'Drop the category, safe only and search filters.',
    apply: 'Keep this filter and go back to the list.',
    cancel: 'Drop the filter and go back to the list.',
    selected: `${formatBytes(bytesOf(selected))} selected. Click or enter deletes it, u clears the selection.`,
    freed: 'Freed for good. Permanent deletes give the space back immediately.',
    trashed: 'Waiting in the trash. The space comes back once the trash is emptied.',
  }
}

/**
 * @name confirmTips
 * @description Tooltip copy for the keys in the delete confirmation, phrased
 * for the mode the run would use.
 *
 * @example
 * confirmTips('trash').confirm // 'Move these to the trash.'
 */
export function confirmTips(mode: DeleteMode) {
  return {
    confirm: mode === 'trash' ? 'Move these to the trash.' : 'Delete these permanently. There is no undo.',
    toggle:
      mode === 'trash'
        ? 'Delete permanently instead of trashing, for this run only.'
        : 'Move to the trash instead of deleting, for this run only.',
    cancel: 'Keep everything and close.',
  }
}

/**
 * @name segmentTip
 * @description Tooltip copy for one segment of the disk bar: the item it
 * stands for and what state it is in.
 *
 * @example
 * segmentTip(item, entry) // '~/projects/editor/target\n14.2 GiB, selected'
 */
export function segmentTip(item: DemoItem, entry: ItemState): string {
  const status =
    entry.status === 'deleted'
      ? 'cleared'
      : !entry.found
        ? 'not found yet'
        : entry.selected
          ? 'selected'
          : 'click to jump to it'
  return `${item.path}\n${formatBytes(item.bytes)}, ${status}`
}
