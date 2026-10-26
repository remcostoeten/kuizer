'use client'

import { demoItems, type Category } from '@/content/demo'
import { formatBytes } from '@/shared/helpers/bytes'
import type { Action } from './reducer'
import { currentId, itemState, type State } from './model'
import { segmentTip } from './tips'

type Props = {
  state: State
  dispatch: (action: Action) => void
}

const ordered = [...demoItems].sort((left, right) => right.bytes - left.bytes)
const total = demoItems.reduce((sum, item) => sum + item.bytes, 0)

const segColor: Record<Category, string> = {
  dependencies: 'bg-t-orange',
  build: 'bg-t-periwinkle',
  cache: 'bg-t-teal',
  temp: 'bg-faint',
}

const segStatus = {
  freed: 'seg-freed',
  unknown: 'opacity-15',
  selected: 'inset-ring inset-ring-fg',
  held: '',
}

export function DiskBar({ state, dispatch }: Props) {
  const current = currentId(state)
  const cleared = demoItems
    .filter((item) => itemState(state, item.id).status === 'deleted')
    .reduce((sum, item) => sum + item.bytes, 0)
  const label =
    cleared > 0
      ? `${formatBytes(cleared)} of ${formatBytes(total)} cleared`
      : `${formatBytes(total)} reclaimable`

  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-line px-3 py-2">
      <div className="flex h-2 flex-1 gap-px" aria-label="Disk space held by each artifact">
        {ordered.map((item) => {
          const entry = itemState(state, item.id)
          const status =
            entry.status === 'deleted'
              ? 'freed'
              : !entry.found
                ? 'unknown'
                : entry.selected
                  ? 'selected'
                  : 'held'
          return (
            <button
              key={item.id}
              type="button"
              tabIndex={-1}
              data-tip={segmentTip(item, entry)}
              aria-label={`${item.path}, ${formatBytes(item.bytes)}`}
              onClick={() => dispatch({ type: 'focusItem', id: item.id })}
              style={{ flex: `${item.bytes} 1 3px` }}
              className={`seg min-w-[3px] rounded-[1px] pointer-fine:hover:scale-y-175 ${status === 'freed' ? '' : segColor[item.category]} ${segStatus[status]} ${current === item.id ? 'scale-y-175' : ''}`}
            />
          )
        })}
      </div>
      <span className="text-muted shrink-0 whitespace-nowrap text-micro">{label}</span>
    </div>
  )
}
