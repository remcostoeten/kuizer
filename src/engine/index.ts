import { Project, SyntaxKind, ts, SourceFile } from 'ts-morph'
import { dirname, isAbsolute, resolve } from 'path'
import fg from 'fast-glob'
import type { AnalysisBoundary, Config } from '../types.js'
import { FileCacheManager } from './cache.js'

/** Initialises the ts-morph project, collects all source files, and runs BFS reachability from resolved entry points. */

type BootstrapProgress =
	| Readonly<{
			phase: 'load-project' | 'collect-files' | 'resolve-entries' | 'complete'
			totalFiles?: number
	  }>
	| Readonly<{
			phase: 'analyze-file'
			file: string
			completed: number
			totalFiles: number
	  }>

type BootstrapOptions = Readonly<{
	onProgress?: (event: BootstrapProgress) => void
}>

export async function bootstrap(config: Config): Promise<{
	project: Project
	reachable: ReadonlySet<string>
	testReachable: ReadonlySet<string>
	all: ReadonlySet<string>
	analysisBoundaries: readonly AnalysisBoundary[]
	cache: FileCacheManager
}>
export async function bootstrap(
	config: Config,
	options: BootstrapOptions
): Promise<{
	project: Project
	reachable: ReadonlySet<string>
	testReachable: ReadonlySet<string>
	all: ReadonlySet<string>
	analysisBoundaries: readonly AnalysisBoundary[]
	cache: FileCacheManager
}>
export async function bootstrap(
	config: Config,
	options: BootstrapOptions = {}
): Promise<{
	project: Project
	reachable: ReadonlySet<string>
	testReachable: ReadonlySet<string>
	all: ReadonlySet<string>
	analysisBoundaries: readonly AnalysisBoundary[]
	cache: FileCacheManager
}> {
	const root = dirname(resolve(config.tsconfig))
	const cache = new FileCacheManager(root)
	options.onProgress?.({ phase: 'load-project' })

	const project = new Project({
		tsConfigFilePath: resolve(config.tsconfig),
		skipFileDependencyResolution: true,
		skipLoadingLibFiles: true,
	})
	await addTestFiles(project, config, root)
	await addWorkspaceFiles(project, config, root) // added this as well to cover monorepo sibling consumers
	options.onProgress?.({ phase: 'collect-files' })
	const ignoredFiles = new Set(config.ignore.files)
	const ignoredPaths = config.ignore.paths.map((path) =>
		isAbsolute(path) ? path : resolve(root, path)
	)

	const files = project
		.getSourceFiles()
		.map((file) => file.getFilePath())
		.filter((file) => !isIgnored(file, config, root, ignoredFiles, ignoredPaths))
	const all = new Set(files)
	options.onProgress?.({ phase: 'resolve-entries', totalFiles: all.size })
	const configuredEntries = await resolveEntries(config, all, root)
	
	const graph = buildGraph(project, all, cache)
	const inferredEntries = inferEntries(all, graph.imported)
	
	const tests = new Set([...all].filter((file) => isTestFile(file.slice(root.length + 1))))
	
	const baseEntries =
		configuredEntries.size > 0 ? configuredEntries : new Set([...inferredEntries].filter(e => !(tests as any).has(e)))
	
	const prodEntries = config.countTestsAsConsumers
		? new Set([...baseEntries, ...tests])
		: new Set([...baseEntries])
	const reachable = bfs(prodEntries, graph.adjacency, all.size, options.onProgress)
	
	const testReachable = bfs(tests, graph.adjacency, all.size)
	// remove anything reachable from prod
	for (const f of reachable) testReachable.delete(f)

	const combinedReachable = new Set([...reachable, ...testReachable])
	const analysisBoundaries = collectAnalysisBoundaries(project, combinedReachable, cache)
	options.onProgress?.({ phase: 'complete', totalFiles: all.size })

	return { project, reachable, testReachable, all, analysisBoundaries, cache }
}

function isIgnored(
	file: string,
	config: Config,
	root: string,
	ignoredFiles: ReadonlySet<string>,
	ignoredPaths: readonly string[]
): boolean {
	const relative = file.slice(root.length + 1)
	if (relative.startsWith('node_modules/')) return true
	if (relative.startsWith('.next/')) return true
	if (file.startsWith(config.outputDir + '/')) return true
	if (!config.countTestsAsConsumers && isTestFile(relative)) return true

	for (const absolute of ignoredPaths) {
		if (file === absolute || file.startsWith(absolute + '/')) {
			return true
		}
	}

	return ignoredFiles.has(relative) || ignoredFiles.has(file)
}

function isTestFile(path: string): boolean {
	return /\.(test|spec)\.[cm]?[jt]sx?$/.test(path)
}

async function addTestFiles(project: Project, config: Config, root: string): Promise<void> {
	if (!config.countTestsAsConsumers) {
		return
	}

	const matches = await fg(
		[
			resolve(root, '**/*.test.{ts,tsx,js,jsx,mts,cts,mjs,cjs}'),
			resolve(root, '**/*.spec.{ts,tsx,js,jsx,mts,cts,mjs,cjs}'),
		],
		{
			absolute: true,
			onlyFiles: true,
			ignore: [`${config.outputDir}/**`, `${resolve(root, 'node_modules')}/**`],
		}
	)

	if (matches.length > 0) {
		project.addSourceFilesAtPaths(matches)
	}
}

async function resolveEntries(
	config: Config,
	all: ReadonlySet<string>,
	root: string
): Promise<ReadonlySet<string>> {
	const patterns =
		config.entrypoints.length > 0
			? config.entrypoints.map((p) => (isAbsolute(p) ? p : resolve(root, p)))
			: [
					resolve(root, 'src/index.ts'),
					resolve(root, 'src/index.tsx'),
					resolve(root, 'index.ts'),
				]

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

function inferEntries(
	all: ReadonlySet<string>,
	imported: ReadonlySet<string>
): ReadonlySet<string> {
	return new Set([...all].filter((file) => !imported.has(file)))
}

function buildGraph(
	project: Project,
	all: ReadonlySet<string>,
	cache: FileCacheManager
): Readonly<{
	adjacency: ReadonlyMap<string, readonly string[]>
	imported: ReadonlySet<string>
}> {
	const adjacency = new Map<string, readonly string[]>()
	const imported = new Set<string>()

	for (const path of all) {
		let adjacent = cache.getCachedFileContext(path)?.adjacent
		if (!adjacent) {
			const sf = project.getSourceFile(path)
			if (!sf) {
				adjacency.set(path, [])
				continue
			}

			adjacent = getAdjacentFiles(sf, all)
			cache.setCachedFileContext(path, { adjacent })
		}

		adjacency.set(path, adjacent)

		for (const next of adjacent) {
			imported.add(next)
		}
	}

	return {
		adjacency,
		imported,
	}
}

function collectAnalysisBoundaries(
	project: Project,
	reachable: ReadonlySet<string>
): readonly AnalysisBoundary[] {
	const analysisBoundaries: AnalysisBoundary[] = []

	for (const path of reachable) {
		const sf = project.getSourceFile(path)
		if (!sf) {
			continue
		}

		recordDynamicImportBoundaries(sf, analysisBoundaries)
	}

	return analysisBoundaries
}

function bfs(
	entries: ReadonlySet<string>,
	adjacency: ReadonlyMap<string, readonly string[]>,
	totalFiles: number,
	onProgress?: (event: BootstrapProgress) => void
): Set<string> {
	const visited = new Set<string>()
	const queue = [...entries]
	let head = 0

	while (head < queue.length) {
		const path = queue[head++]!
		if (visited.has(path)) continue
		visited.add(path)
		onProgress?.({
			phase: 'analyze-file',
			file: path,
			completed: visited.size,
			totalFiles,
		})

		for (const next of adjacency.get(path) ?? []) {
			if (!visited.has(next)) queue.push(next)
		}
	}

	return visited
}

function getAdjacentFiles(
	sourceFile: {
		getImportDeclarations(): Array<{
			getModuleSpecifierSourceFile(): { getFilePath(): string } | undefined
		}>
		getExportDeclarations(): Array<{
			getModuleSpecifierSourceFile(): { getFilePath(): string } | undefined
		}>
		getFilePath(): string
		getDescendantsOfKind(kind: SyntaxKind.CallExpression): Array<{
			getExpression(): { getKind(): SyntaxKind }
			getArguments(): Array<{
				getKind(): SyntaxKind
				getLiteralText?: () => string
				getStart(): number
				getSourceFile(): {
					getFilePath(): string
					getLineAndColumnAtPos(pos: number): { line: number; column: number }
				}
			}>
		}>
	},
	all: ReadonlySet<string>
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

	for (const call of sourceFile.getDescendantsOfKind(SyntaxKind.CallExpression)) {
		if (call.getExpression().getKind() !== SyntaxKind.ImportKeyword) continue

		const specifier = call.getArguments()[0]
		if (!specifier) continue

		if (
			specifier.getKind() === SyntaxKind.StringLiteral ||
			specifier.getKind() === SyntaxKind.NoSubstitutionTemplateLiteral
		) {
			const resolved = resolveDynamicImport(
				sourceFile.getFilePath(),
				specifier.getLiteralText!(),
				all
			)

			if (resolved) {
				adjacent.add(resolved)
			}

			continue
		}
	}

	return [...adjacent]
}

function recordDynamicImportBoundaries(
	sourceFile: {
		getDescendantsOfKind(kind: SyntaxKind.CallExpression): Array<{
			getExpression(): { getKind(): SyntaxKind }
			getArguments(): Array<{
				getKind(): SyntaxKind
				getStart(): number
				getSourceFile(): {
					getLineAndColumnAtPos(pos: number): { line: number; column: number }
				}
			}>
		}>
		getFilePath(): string
	},
	analysisBoundaries: AnalysisBoundary[]
): void {
	for (const call of sourceFile.getDescendantsOfKind(SyntaxKind.CallExpression)) {
		if (call.getExpression().getKind() !== SyntaxKind.ImportKeyword) continue

		const specifier = call.getArguments()[0]
		if (!specifier) continue

		if (
			specifier.getKind() === SyntaxKind.StringLiteral ||
			specifier.getKind() === SyntaxKind.NoSubstitutionTemplateLiteral
		) {
			continue
		}

		const { line, column } = specifier
			.getSourceFile()
			.getLineAndColumnAtPos(specifier.getStart())
		analysisBoundaries.push({
			file: sourceFile.getFilePath(),
			line,
			col: column,
			kind: 'dynamic-import',
			message: 'Non-literal dynamic import cannot be resolved statically',
		})
	}
}

function resolveDynamicImport(
	fromFile: string,
	specifier: string,
	all: ReadonlySet<string>
): string | undefined {
	if (!specifier.startsWith('.')) {
		return undefined
	}

	const base = resolve(dirname(fromFile), specifier)
	const withoutJsExt = base.replace(/\.(mjs|cjs|js|jsx)$/, '')
	const candidates = [
		base,
		withoutJsExt,
		`${base}.ts`,
		`${base}.tsx`,
		`${withoutJsExt}.ts`,
		`${withoutJsExt}.tsx`,
		resolve(base, 'index.ts'),
		resolve(base, 'index.tsx'),
		resolve(withoutJsExt, 'index.ts'),
		resolve(withoutJsExt, 'index.tsx'),
	]

	return candidates.find((candidate) => all.has(candidate))
}
