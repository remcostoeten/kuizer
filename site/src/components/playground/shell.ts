import { demoCaches, demoRoot, projectNames, scanTotals } from '@/content/demo'
import { installOptions } from '@/content/install'
import { version } from '@/content/site'
import { formatBytes, formatCount } from '@/shared/helpers/bytes'
import {
  bytesOf,
  itemById,
  itemState,
  onDiskIds,
  startScan,
  updateItems,
  type HistoryRun,
  type ShellLine,
  type State,
} from './model'

type Result = {
  state: State
  lines: ShellLine[]
}

const historyPath = '~/.kuizer-history.json'

function plain(text: string): ShellLine {
  return { text, tone: 'plain' }
}

function muted(text: string): ShellLine {
  return { text, tone: 'muted' }
}

function warn(text: string): ShellLine {
  return { text, tone: 'warn' }
}

function ok(text: string): ShellLine {
  return { text, tone: 'ok' }
}

function clock(iso: string): string {
  const date = new Date(iso)
  const hours = String(date.getHours()).padStart(2, '0')
  const minutes = String(date.getMinutes()).padStart(2, '0')
  return `today ${hours}:${minutes}`
}

function plural(count: number, word: string): string {
  return `${count} ${word}${count === 1 ? '' : 's'}`
}

function help(): ShellLine[] {
  return [
    muted('A small shell that lives in this page. It knows:'),
    plain(''),
    plain(`  kuizer ${demoRoot}          open the interactive list again`),
    plain(`  kuizer scan ${demoRoot}     print what is on the fake disk`),
    plain('  kuizer history              every deletion made on this page'),
    plain('  kuizer restore --last       put the last run back from the trash'),
    plain('  kuizer restore s1           put one run back by id'),
    plain('  kuizer cache                global caches the real tool knows'),
    plain('  ls, clear, help'),
    plain(''),
    muted('Nothing here touches your machine. Install the real thing below.'),
  ]
}

function scan(state: State): ShellLine[] {
  const ids = onDiskIds(state)
  if (ids.length === 0) {
    return [muted(`nothing found under ${demoRoot}, every artifact is in the trash`)]
  }
  const items = ids.map(itemById).sort((left, right) => right.bytes - left.bytes)
  const lines = items.map((item) => {
    const badge = item.safety === 'safe' ? '● safe ' : '▲ check'
    return {
      text: `  ${formatBytes(item.bytes).padStart(10)}  ${badge}  ${item.category.padEnd(12)}  ${item.rule.padEnd(14)}  ${item.path}`,
      tone: item.safety === 'safe' ? 'plain' : 'warn',
    } satisfies ShellLine
  })
  const safe = items.filter((item) => item.safety === 'safe')
  const review = items.filter((item) => item.safety === 'review')
  return [
    ...lines,
    plain(''),
    muted(
      `${plural(items.length, 'item')} in ${formatCount(scanTotals.dirs)} folders: ${formatBytes(bytesOf(safe.map((item) => item.id)))} safe, ${formatBytes(bytesOf(review.map((item) => item.id)))} to review`,
    ),
    muted('a scan never deletes anything; the list or `kuizer clean` does, after a confirmation'),
  ]
}

function caches(): ShellLine[] {
  const total = demoCaches.reduce((sum, cache) => sum + cache.bytes, 0)
  return [
    ...demoCaches.map((cache) =>
      plain(`  ${formatBytes(cache.bytes).padStart(10)}  ${cache.tool.padEnd(12)}  ${cache.path}`),
    ),
    plain(''),
    muted(
      `${formatBytes(total)} in ${demoCaches.length} caches, \`kuizer cache --clean --select npm\` removes one`,
    ),
  ]
}

function runLine(run: HistoryRun): ShellLine {
  const where = run.restored ? 'restored' : run.mode === 'trash' ? 'in the trash' : 'deleted permanently'
  return plain(
    `  ${run.id.padEnd(4)} ${clock(run.at).padEnd(12)} ${plural(run.ids.length, 'item').padEnd(9)} ${formatBytes(bytesOf(run.ids)).padStart(10)}  ${run.mode.padEnd(9)} ${where}`,
  )
}

function history(state: State): ShellLine[] {
  if (state.runs.length === 0) {
    return [muted(`nothing deleted yet, history is kept in ${historyPath}`)]
  }
  const counted = state.runs.filter((run) => !run.restored)
  const restored = state.runs.filter((run) => run.restored)
  const countedIds = counted.flatMap((run) => run.ids)
  const trashIds = counted.filter((run) => run.mode === 'trash').flatMap((run) => run.ids)
  const freedIds = counted.filter((run) => run.mode === 'permanent').flatMap((run) => run.ids)
  const first = state.runs[0]
  const lines = [
    ...state.runs.map(runLine),
    plain(''),
    plain(
      `${formatBytes(bytesOf(countedIds))} removed across ${plural(countedIds.length, 'item')} in ${plural(counted.length, 'run')} since ${first ? clock(first.at) : '-'}`,
    ),
  ]
  if (restored.length > 0) {
    const ids = restored.flatMap((run) => run.ids)
    lines.push(
      plain(
        `${formatBytes(bytesOf(ids))} across ${plural(ids.length, 'item')} was restored and is not counted`,
      ),
    )
  }
  if (trashIds.length > 0) {
    if (freedIds.length > 0) {
      lines.push(
        plain(
          `${formatBytes(bytesOf(freedIds))} of that is freed, deleted permanently or emptied from the trash`,
        ),
      )
    }
    lines.push(
      plain(
        `${formatBytes(bytesOf(trashIds))} is still in the trash across ${plural(trashIds.length, 'item')} and frees once the trash is emptied, \`kuizer restore ID\` puts it back`,
      ),
    )
  } else if (countedIds.length > 0) {
    lines.push(plain('all of it is freed, none of it is still in the trash'))
  }
  lines.push(muted(historyPath))
  return lines
}

function restore(state: State, target: string): Result {
  const run =
    target === '--last' || target === ''
      ? [...state.runs].reverse().find((entry) => !entry.restored)
      : state.runs.find((entry) => entry.id === target)
  if (!run) {
    const reason =
      state.runs.length === 0 ? 'nothing to restore, the history is empty' : `no run ${target} in the history`
    return { state, lines: [warn(reason)] }
  }
  if (run.restored) {
    return { state, lines: [muted(`${run.id} was already restored`)] }
  }
  if (run.mode === 'permanent') {
    return {
      state,
      lines: [warn(`${run.id} was deleted permanently, there is nothing in the trash to put back`)],
    }
  }
  const restorable = run.ids.filter((id) => itemState(state, id).status === 'deleted')
  const lines = restorable.map((id, index) =>
    ok(
      `  restored     ${`${run.id}:${index}`.padEnd(8)}  ${formatBytes(itemById(id).bytes).padStart(10)}  ${itemById(id).path}`,
    ),
  )
  const bytes = bytesOf(restorable)
  const runs = state.runs.map((entry) => (entry.id === run.id ? { ...entry, restored: true } : entry))
  const next = updateItems(state, restorable, { status: 'ready', selected: false })
  return {
    state: { ...next, runs, trashed: Math.max(state.trashed - bytes, 0) },
    lines: [
      ...lines,
      plain(''),
      plain(`${plural(restorable.length, 'item')} restored (${formatBytes(bytes)}), 0 skipped`),
      muted(`run kuizer ${demoRoot} again and they are back in the list`),
    ],
  }
}

function install(command: string): ShellLine[] {
  return [
    muted(`On your machine \`${command}\` installs kuizer ${version}.`),
    muted('This shell only runs the demo. Copy the command from the install list below.'),
  ]
}

/**
 * @name runShell
 * @description Runs one line typed at the fake prompt and returns the next
 * state with the output appended. Relaunching the list hands back a state
 * whose screen is the list, with a fresh scan queued.
 *
 * @example
 * const next = runShell(state, 'kuizer history')
 */
export function runShell(state: State, line: string): State {
  const trimmed = line.trim()
  const words = trimmed.split(/\s+/)
  const [program = '', command = '', argument = ''] = words
  const echo: ShellLine = { text: `$ ${trimmed}`, tone: 'prompt' }
  const shellHistory = trimmed === '' ? state.shellHistory : [...state.shellHistory, trimmed]
  const echoed: State = { ...state, shell: [...state.shell, echo], shellHistory }

  function reply(lines: ShellLine[], next: State = echoed): State {
    return { ...next, shell: [...echoed.shell, ...lines] }
  }

  if (trimmed === '') return echoed
  if (trimmed === 'clear') return { ...echoed, shell: [] }
  if (trimmed === 'help' || trimmed === 'kuizer --help' || trimmed === 'kuizer -h') return reply(help())
  if (trimmed === 'ls') return reply([plain(projectNames.join('   '))])
  if (program === 'cd') return reply([muted(`this shell stays in ${demoRoot}`)])
  if (program === 'exit' || program === 'logout')
    return reply([muted('there is nothing to exit, this shell is the page')])
  if (
    installOptions.some((option) => option.command === trimmed) ||
    program === 'npx' ||
    program === 'cargo'
  ) {
    return reply(install(trimmed))
  }
  if (program !== 'kuizer') {
    return reply([
      warn(`command not found: ${program}`),
      muted('type help for the commands this shell knows'),
    ])
  }
  if (command === '--version' || command === '-V') return reply([plain(`kuizer ${version}`)])
  if (command === '' || command.startsWith('~') || command === '.' || command.startsWith('/')) {
    return startScan(echoed)
  }
  if (command === 'scan') return reply(scan(echoed))
  if (command === 'history') return reply(history(echoed))
  if (command === 'cache') return reply(caches())
  if (command === 'restore') {
    const result = restore(echoed, argument)
    return reply(result.lines, result.state)
  }
  if (command === 'clean') {
    return reply([
      muted(
        `kuizer clean ${demoRoot} removes from the terminal without the list; in this demo, use the list.`,
      ),
      muted(`type kuizer ${demoRoot} to open it`),
    ])
  }
  if (command === 'dashboard' || command === 'duplicates' || command === 'code') {
    return reply([
      muted(`kuizer ${command} exists in the real tool, not in this demo. The README on GitHub shows it.`),
    ])
  }
  return reply([
    warn(`unknown command: kuizer ${command}`),
    muted('type help for the commands this shell knows'),
  ])
}
