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
export const authorUrl = 'https://remcostoeten.com'
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

export const tagline = 'Reclaim your disk from build output, caches and duplicates.'
export const headline = ['Reclaim Your Disk', 'From'] as const
export const headlineTargets = [
  { word: 'build output', size: 2.1, unit: 'GB' },
  { word: 'node_modules', size: 1.4, unit: 'GB' },
  { word: 'tool caches', size: 3.8, unit: 'GB' },
  { word: 'toolchains', size: 6.2, unit: 'GB' },
  { word: 'duplicates', size: 900, unit: 'MB' },
] as const
export const quickRuns = ['npx kuizer', 'cargo install kuizer'] as const
export const subtitle = [
  'Your dev environment leaves behind gigabytes of stale bloat.',
  'Find it and clean it up from the desktop app or the TUI.',
] as const
export const metaDescription =
  'Find build output, dependency folders and tool caches, see what recreates them, and move the ones you pick to the trash. Terminal UI and desktop app.'
export const description =
  'One scan lists dependency folders, build output, global caches, old toolchains, Docker leftovers, git repos and duplicate files with their sizes, and marks which ones the next build recreates. You tick rows, the trash takes them, and kuizer restore brings a run back.'
