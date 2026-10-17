export type Feature = {
  icon: 'check' | 'trash' | 'shield' | 'git' | 'recheck' | 'history'
  title: string
  body: string
}

export type Showcase = {
  id: string
  eyebrow: string
  title: string
  body: string
  image: string
  alt: string
  windowTitle: string
}

export const showcases: Showcase[] = [
  {
    id: 'safety',
    eyebrow: 'Safety checks',
    title: 'Checked against its project',
    body: 'A dist folder counts only next to a package.json, and a target folder only next to a Cargo.toml with the cache tag inside. Every result is marked safe or review before you see it.',
    image: '/screenshots/artifacts.png',
    alt: 'The project artifacts table with size, safety, category, rule and path columns',
    windowTitle: 'kuizer ~/projects · artifacts',
  },
  {
    id: 'trash',
    eyebrow: 'Trash, not rm',
    title: 'Every deletion is reversible',
    body: 'Items go to the trash, never straight to rm. When a review item is in the selection the confirmation asks you to type the count, and kuizer restore puts a whole run back without overwriting anything.',
    image: '/screenshots/confirm.png',
    alt: 'The confirmation dialog listing seven items and asking to type the count',
    windowTitle: 'kuizer ~/projects · confirm',
  },
  {
    id: 'caches',
    eyebrow: 'Global caches',
    title: 'The caches outside your projects',
    body: 'npm, pnpm, yarn, bun, deno, cargo, pip, uv, Go, Gradle, Maven and Playwright caches are listed with their sizes and can be removed one at a time.',
    image: '/screenshots/caches.png',
    alt: 'The global caches list with one row per package manager',
    windowTitle: 'kuizer ~/projects · caches',
  },
  {
    id: 'duplicates',
    eyebrow: 'Duplicates',
    title: 'Identical files, one copy kept',
    body: 'Files with the same contents are matched by size and BLAKE3 hash. kuizer never deletes a duplicate on its own: you choose which copy stays.',
    image: '/screenshots/duplicates.png',
    alt: 'The duplicates view grouping files with identical contents',
    windowTitle: 'kuizer ~/projects · duplicates',
  },
  {
    id: 'history',
    eyebrow: 'History',
    title: 'Every run on record',
    body: 'Each run lands in ~/.kuizer-history.json with what was freed and what is still in the trash. kuizer history lists them and kuizer restore s4 brings one back.',
    image: '/screenshots/history.png',
    alt: 'The history view listing past runs with freed space and trash state',
    windowTitle: 'kuizer ~/projects · history',
  },
]

export const safeguards: Feature[] = [
  {
    icon: 'shield',
    title: 'Never touches what it cannot regenerate',
    body: 'Browser profiles, ~/.ssh, ~/.gnupg, keyrings, global installs and editor extensions are skipped. Anything else in a hidden folder is marked review.',
  },
  {
    icon: 'recheck',
    title: 'Rechecks every path before removal',
    body: 'No symlinks, no mount points, no home or system directories. The item must still be the directory it scanned.',
  },
  {
    icon: 'git',
    title: 'Warns on uncommitted changes',
    body: 'A project with a dirty working tree gets a warning in the list and again in the delete confirmation.',
  },
]
