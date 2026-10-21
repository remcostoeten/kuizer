export type Highlight = {
  art: 'rules' | 'sizes' | 'duplicates' | 'trash' | 'history' | 'keys'
  title: readonly [string, string]
  body: string
}

export const highlightsEyebrow = 'What it does'
export const highlightsTitle = ['Finds the weight,', 'deletes what you tick.'] as const
export const highlightsLead =
  'Dependency folders, build output, tool caches and duplicates, found by rules that check the project next to them. Nothing leaves the disk until you tick it and confirm.'

export const highlights: Highlight[] = [
  {
    art: 'rules',
    title: ['Finds what', 'regenerates'],
    body: 'Build folders, checked against the manifest beside them.',
  },
  {
    art: 'sizes',
    title: ['Sizes as', 'it scans'],
    body: 'Sizes stream in while the scan runs, largest first.',
  },
  {
    art: 'duplicates',
    title: ['Duplicates', 'by hash'],
    body: 'Identical files grouped by BLAKE3. You pick the copy that stays.',
  },
  {
    art: 'trash',
    title: ['Trash,', 'never rm'],
    body: 'Everything goes to the trash first. Permanent delete is opt in.',
  },
  {
    art: 'history',
    title: ['History', 'and restore'],
    body: 'Every run is logged. One command puts a whole run back.',
  },
  {
    art: 'keys',
    title: ['Keyboard', 'first'],
    body: 'Every action has a key. The mouse is optional.',
  },
]
