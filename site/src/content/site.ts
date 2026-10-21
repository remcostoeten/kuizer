export const siteUrl = process.env.NEXT_PUBLIC_SITE_URL ?? 'https://kuizer.dev'
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

function requireVersion(value: string | undefined) {
  if (!value)
    throw new Error('NEXT_PUBLIC_KUIZER_VERSION is unset, next.config.ts reads it from ../Cargo.toml')
  return value
}

export const version = requireVersion(process.env.NEXT_PUBLIC_KUIZER_VERSION)

export const tagline = 'Reclaim your disk from build output, caches and duplicates.'
export const headline = ['Reclaim Your Disk', 'From'] as const
export const headlineTargets = [
  'build output',
  'node_modules',
  'tool caches',
  'Docker',
  'toolchains',
  'duplicates',
  'git repos',
] as const
export const quickRuns = ['npx kuizer', 'cargo install kuizer'] as const
export const subtitle =
  'One scan for dependencies, build output, global caches, toolchains, Docker, git repos and duplicates.'
export const description =
  'One scan lists dependency folders, build output, global caches, old toolchains, Docker leftovers, git repos and duplicate files with their sizes, and marks which ones the next build recreates. You tick rows, the trash takes them, and kuizer restore brings a run back.'
