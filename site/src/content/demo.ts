import { GiB, MiB } from '@/shared/helpers/bytes'

export type Category = 'dependencies' | 'build' | 'cache' | 'temp'
export type Safety = 'safe' | 'review'

export type DemoItem = {
  id: string
  path: string
  project: string
  rule: string
  ecosystem: string
  category: Category
  safety: Safety
  bytes: number
  entries: number
  age: string
}

export type DemoCache = {
  tool: string
  path: string
  bytes: number
}

export const demoRoot = '~/projects'
export const demoCommand = `kuizer ${demoRoot}`
export const categories: Category[] = ['dependencies', 'build', 'cache', 'temp']

export const demoItems: DemoItem[] = [
  {
    id: 'editor-target',
    path: '~/projects/editor/target',
    project: '~/projects/editor',
    rule: 'cargo-target',
    ecosystem: 'rust',
    category: 'build',
    safety: 'safe',
    bytes: 14.2 * GiB,
    entries: 48_211,
    age: '2d',
  },
  {
    id: 'api-target',
    path: '~/projects/api/target',
    project: '~/projects/api',
    rule: 'cargo-target',
    ecosystem: 'rust',
    category: 'build',
    safety: 'safe',
    bytes: 9.4 * GiB,
    entries: 31_004,
    age: '5d',
  },
  {
    id: 'game-target',
    path: '~/projects/game/target',
    project: '~/projects/game',
    rule: 'cargo-target',
    ecosystem: 'rust',
    category: 'build',
    safety: 'safe',
    bytes: 3.9 * GiB,
    entries: 12_870,
    age: '3w',
  },
  {
    id: 'mobile-node-modules',
    path: '~/projects/mobile/node_modules',
    project: '~/projects/mobile',
    rule: 'node-modules',
    ecosystem: 'node',
    category: 'dependencies',
    safety: 'safe',
    bytes: 2.3 * GiB,
    entries: 141_302,
    age: '1d',
  },
  {
    id: 'storefront-node-modules',
    path: '~/projects/storefront/node_modules',
    project: '~/projects/storefront',
    rule: 'node-modules',
    ecosystem: 'node',
    category: 'dependencies',
    safety: 'safe',
    bytes: 1.8 * GiB,
    entries: 98_440,
    age: 'today',
  },
  {
    id: 'monorepo-node-modules',
    path: '~/projects/monorepo/node_modules',
    project: '~/projects/monorepo',
    rule: 'node-modules',
    ecosystem: 'node',
    category: 'dependencies',
    safety: 'safe',
    bytes: 1.6 * GiB,
    entries: 90_330,
    age: '6d',
  },
  {
    id: 'docs-cache',
    path: '~/projects/docs/.cache',
    project: '~/projects/docs',
    rule: 'js-tool-cache',
    ecosystem: 'node',
    category: 'cache',
    safety: 'safe',
    bytes: 1.4 * GiB,
    entries: 6_212,
    age: '4mo',
  },
  {
    id: 'dashboard-node-modules',
    path: '~/projects/dashboard/node_modules',
    project: '~/projects/dashboard',
    rule: 'node-modules',
    ecosystem: 'node',
    category: 'dependencies',
    safety: 'safe',
    bytes: 1.1 * GiB,
    entries: 70_118,
    age: '9d',
  },
  {
    id: 'legacy-build',
    path: '~/projects/legacy-api/build',
    project: '~/projects/legacy-api',
    rule: 'js-build',
    ecosystem: 'node',
    category: 'build',
    safety: 'review',
    bytes: 620 * MiB,
    entries: 2_140,
    age: '1y',
  },
  {
    id: 'storefront-next',
    path: '~/projects/storefront/.next',
    project: '~/projects/storefront',
    rule: 'next-build',
    ecosystem: 'nextjs',
    category: 'build',
    safety: 'safe',
    bytes: 412 * MiB,
    entries: 3_880,
    age: 'today',
  },
  {
    id: 'monorepo-turbo',
    path: '~/projects/monorepo/.turbo',
    project: '~/projects/monorepo',
    rule: 'turbo-cache',
    ecosystem: 'turborepo',
    category: 'cache',
    safety: 'safe',
    bytes: 388 * MiB,
    entries: 1_207,
    age: '6d',
  },
  {
    id: 'scratch-cache',
    path: '~/projects/scratch/.cache',
    project: '~/projects/scratch',
    rule: 'js-tool-cache',
    ecosystem: 'node',
    category: 'cache',
    safety: 'review',
    bytes: 256 * MiB,
    entries: 918,
    age: '2y',
  },
  {
    id: 'android-gradle',
    path: '~/projects/android-app/.gradle',
    project: '~/projects/android-app',
    rule: 'gradle-cache',
    ecosystem: 'gradle',
    category: 'cache',
    safety: 'safe',
    bytes: 212 * MiB,
    entries: 1_540,
    age: '3mo',
  },
  {
    id: 'old-site-dist',
    path: '~/projects/old-site/dist',
    project: '~/projects/old-site',
    rule: 'js-dist',
    ecosystem: 'node',
    category: 'build',
    safety: 'safe',
    bytes: 88 * MiB,
    entries: 412,
    age: '8mo',
  },
  {
    id: 'mobile-coverage',
    path: '~/projects/mobile/coverage',
    project: '~/projects/mobile',
    rule: 'js-coverage',
    ecosystem: 'node',
    category: 'temp',
    safety: 'safe',
    bytes: 41 * MiB,
    entries: 268,
    age: '1d',
  },
]

export const discoveryOrder = [
  'android-gradle',
  'api-target',
  'dashboard-node-modules',
  'docs-cache',
  'editor-target',
  'game-target',
  'legacy-build',
  'mobile-coverage',
  'mobile-node-modules',
  'monorepo-node-modules',
  'monorepo-turbo',
  'old-site-dist',
  'scratch-cache',
  'storefront-next',
  'storefront-node-modules',
]

export const uncommittedChanges: Record<string, number> = {
  '~/projects/editor': 3,
  '~/projects/storefront': 1,
}

export const scanTotals = { dirs: 2_140, files: 184_230, seconds: 2.6 }
export const diskFree = 41.2 * GiB

export const demoCaches: DemoCache[] = [
  { tool: 'cargo', path: '~/.cargo/registry', bytes: 6.1 * GiB },
  { tool: 'pnpm', path: '~/.local/share/pnpm/store', bytes: 3.8 * GiB },
  { tool: 'gradle', path: '~/.gradle/caches', bytes: 2.2 * GiB },
  { tool: 'playwright', path: '~/.cache/ms-playwright', bytes: 1.9 * GiB },
  { tool: 'npm', path: '~/.npm/_cacache', bytes: 1.2 * GiB },
  { tool: 'uv', path: '~/.cache/uv', bytes: 1.1 * GiB },
  { tool: 'bun', path: '~/.bun/install/cache', bytes: 820 * MiB },
  { tool: 'pip', path: '~/.cache/pip', bytes: 640 * MiB },
  { tool: 'go', path: '~/go/pkg/mod/cache', bytes: 512 * MiB },
]

export const projectNames = [
  'android-app',
  'api',
  'dashboard',
  'docs',
  'editor',
  'game',
  'legacy-api',
  'mobile',
  'monorepo',
  'old-site',
  'scratch',
  'storefront',
]
