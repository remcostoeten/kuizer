import { Project } from 'ts-morph'
import { dirname, isAbsolute, resolve } from 'path'
import fg from 'fast-glob'
import type { Config } from '../types.js'

/** Initialises the ts-morph project, collects all source files, and runs BFS reachability from resolved entry points. */

export async function bootstrap(config: Config): Promise<{ project: Project; reachable: ReadonlySet<string>; all: ReadonlySet<string> }> {
  const root = dirname(resolve(config.tsconfig))

  const project = new Project({
    tsConfigFilePath: resolve(config.tsconfig),
  })

  const files = project
    .getSourceFiles()
    .map(file => file.getFilePath())
    .filter(file => !isIgnored(file, config, root))
  const all = new Set(files)
  const configuredEntries = await resolveEntries(config, all, root)
  const entries = configuredEntries.size > 0 ? configuredEntries : inferEntries(project, all)
  const reachable = bfs(project, all, entries)

  return { project, reachable, all }
}

function isIgnored(file: string, config: Config, root: string): boolean {
  const relative = file.slice(root.length + 1)
  if (relative.startsWith('node_modules/')) return true
  if (file.startsWith(config.outputDir + '/')) return true
  if (/\.(test|spec)\.[cm]?[jt]sx?$/.test(relative)) return true

  for (const path of config.ignore.paths) {
    const absolute = isAbsolute(path) ? path : resolve(root, path)
    if (file === absolute || file.startsWith(absolute + '/')) {
      return true
    }
  }

  return config.ignore.files.includes(relative) || config.ignore.files.includes(file)
}

async function resolveEntries(config: Config, all: ReadonlySet<string>, root: string): Promise<ReadonlySet<string>> {
  const patterns = config.entrypoints.length > 0
    ? config.entrypoints.map(p => isAbsolute(p) ? p : resolve(root, p))
    : [resolve(root, 'src/index.ts'), resolve(root, 'src/index.tsx'), resolve(root, 'index.ts')]

  const entries = new Set<string>()

  for (const pattern of patterns) {
    if (fg.isDynamicPattern(pattern)) {
      const matches = await fg(pattern, {
        absolute: true,
        onlyFiles: true,
      })

      for (const match of matches) {
        if (all.has(match)) {
          entries.add(match)
        }
      }

      continue
    }

    if (all.has(pattern)) {
      entries.add(pattern)
    }
  }

  return entries
}

function inferEntries(project: Project, all: ReadonlySet<string>): ReadonlySet<string> {
  const imported = new Set<string>()

  for (const path of all) {
    const sf = project.getSourceFile(path)
    if (!sf) continue

    for (const next of getAdjacentFiles(sf, all)) {
      imported.add(next)
    }
  }

  return new Set([...all].filter(file => !imported.has(file)))
}

function bfs(project: Project, all: ReadonlySet<string>, entries: ReadonlySet<string>): ReadonlySet<string> {
  const visited = new Set<string>()
  const queue = [...entries]

  while (queue.length > 0) {
    const path = queue.shift()!
    if (visited.has(path)) continue
    visited.add(path)

    const sf = project.getSourceFile(path)
    if (!sf) continue

    for (const next of getAdjacentFiles(sf, all)) {
      if (!visited.has(next)) queue.push(next)
    }
  }

  return visited
}

function getAdjacentFiles(
  sourceFile: {
    getImportDeclarations(): Array<{ getModuleSpecifierSourceFile(): { getFilePath(): string } | undefined }>
    getExportDeclarations(): Array<{ getModuleSpecifierSourceFile(): { getFilePath(): string } | undefined }>
  },
  all: ReadonlySet<string>,
): string[] {
  const adjacent = new Set<string>()

  for (const decl of sourceFile.getImportDeclarations()) {
    const resolved = decl.getModuleSpecifierSourceFile()
    if (!resolved) continue

    const next = resolved.getFilePath()
    if (all.has(next)) {
      adjacent.add(next)
    }
  }

  for (const decl of sourceFile.getExportDeclarations()) {
    const resolved = decl.getModuleSpecifierSourceFile()
    if (!resolved) continue

    const next = resolved.getFilePath()
    if (all.has(next)) {
      adjacent.add(next)
    }
  }

  return [...adjacent]
}
