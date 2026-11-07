export const siteUrl = process.env.NEXT_PUBLIC_SITE_URL ?? 'https://kuizer.app'
export const repoUrl = 'https://github.com/remcostoeten/kuizer'
export const releasesUrl = `${repoUrl}/releases`
export const latestReleaseUrl = `${repoUrl}/releases/latest`
export const issuesUrl = `${repoUrl}/issues`
export const docsUrl = `${repoUrl}#readme`
export const configDocsUrl = `${repoUrl}/blob/master/docs/config.example.toml`
export const designDocsUrl = `${repoUrl}/blob/master/docs/design.md`
export const licenseUrl = `${repoUrl}/blob/master/LICENSE`
export const cratesUrl = 'https://crates.io/crates/kuizer'
export const npmUrl = 'https://www.npmjs.com/package/kuizer'
export const authorName = 'Remco Stoeten'
export const authorUrl = 'https://remcostoeten.com'
export const githubProfileUrl = 'https://github.com/remcostoeten'
export const siteName = 'kuizer'
export const registries = [
  { name: 'npm', url: npmUrl },
  { name: 'crates.io', url: cratesUrl },
] as const

function requireVersion(value: string | undefined) {
  if (!value)
    throw new Error('NEXT_PUBLIC_KUIZER_VERSION is unset, next.config.ts reads it from ../Cargo.toml')
  return value
}

export const version = requireVersion(process.env.NEXT_PUBLIC_KUIZER_VERSION)

export const keywords = [
  'kuizer',
  'disk cleanup',
  'delete node_modules',
  'cargo target',
  'python venv',
  'tool caches',
  'duplicate files',
  'disk usage',
  'terminal UI',
  'Linux',
  'macOS',
]
export const metaTitle = 'kuizer: delete node_modules, Cargo target, .venv and caches'
export const tagline = 'Reclaim your disk from build output, caches and duplicates.'
export const headline = ['Find the gigabytes', 'your builds left behind.'] as const
export const quickRuns = ['npx kuizer', 'cargo install kuizer'] as const
export const subtitle = [
  'node_modules, target, .venv, caches, old toolchains and duplicates.',
  'One scan lists them with sizes. You tick rows, the trash takes them.',
] as const
export const metaDescription =
  'Finds every node_modules, Cargo target, .venv and tool cache under your projects and moves the ones you pick to the trash. Open source, for Linux and macOS.'
export const description =
  'One scan lists dependency folders, build output, global caches, old toolchains, Docker leftovers, git repos and duplicate files with their sizes, and marks which ones the next build recreates. You tick rows, the trash takes them, and kuizer restore brings a run back.'
