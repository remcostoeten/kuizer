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
export const version = '0.1.0'

export const tagline = 'Reclaim your disk from node_modules, target and .next.'
export const headline = ['Reclaim Your Disk', 'From node_modules'] as const
export const subtitle = 'One scan for node_modules, target, .next and your global caches.'
export const description =
  'One scan lists every node_modules, target, .next and global cache with its size and marks which ones the next build recreates. You tick rows, the trash takes them, and kuizer restore brings a run back.'
