export type Principle = {
  title: string
  body: string
}

export const principles: Principle[] = [
  {
    title: 'Scanning never deletes',
    body: 'Nothing leaves the disk until you tick it and confirm.',
  },
  {
    title: 'Trash, not rm',
    body: 'Every deletion is reversible, and kuizer restore puts a whole run back.',
  },
  {
    title: 'Checked against the project',
    body: 'A dist folder counts only next to a package.json.',
  },
]
