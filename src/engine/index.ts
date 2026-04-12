import { Project, SyntaxKind, ts } from 'ts-morph'
import { dirname, isAbsolute, resolve } from 'path'
import fg from 'fast-glob'
import { minimatch } from 'minimatch'
import type { AnalysisBoundary, Config } from '../types.js'

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
	all: ReadonlySet<string>
	analysisBoundaries: readonly AnalysisBoundary[]
}>
export async function bootstrap(
	config: Config,
	options: BootstrapOptions
): Promise<{
	project: Project
	reachable: ReadonlySet<string>
	all: ReadonlySet<string>
	analysisBoundaries: readonly AnalysisBoundary[]
}>
export async function bootstrap(
	config: Config,
	options: BootstrapOptions = {}
): Promise<{
	project: Project
	reachable: ReadonlySet<string>
	all: ReadonlySet<string>
	analysisBoundaries: readonly AnalysisBoundary[]
}> {
	const root = dirname(resolve(config.tsconfig))
	options.onProgress?.({ phase: 'load-project' })

	const project = new Project({
		tsConfigFilePath: resolve(config.tsconfig),
		skipFileDependencyResolution: true,
		skipLoadingLibFiles: true,
	})
	await addTestFiles(project, config, root)
	options.onProgress?.({ phase: 'collect-files' })
	const ignoredFiles = new Set(config.ignore.files)
	const ignoredPaths = config.ignore.paths.map((path) => {
		if (path.includes('**')) {
			return path
		}
		return isAbsolute(path) ? path : resolve(root, path)
	})

	const files = project
		.getSourceFiles()
		.map((file) => file.getFilePath())
		.filter((file) => !isIgnored(file, config, root, ignoredFiles, ignoredPaths))
	const all = new Set(files)
	options.onProgress?.({ phase: 'resolve-entries', totalFiles: all.size })
	const configuredEntries = await resolveEntries(config, all, root)
	const tests = config.countTestsAsConsumers
		? new Set([...all].filter((file) => isTestFile(file.slice(root.length + 1))))
		: new Set<string>()
	const graph = buildGraph(project, all)
	const baseEntries =
		configuredEntries.size > 0 ? configuredEntries : inferEntries(all, graph.imported)
	const entries = new Set([...baseEntries, ...tests])
	const reachable = bfs(entries, graph.adjacency, all.size, options.onProgress)
	const analysisBoundaries = collectAnalysisBoundaries(project, reachable)
	options.onProgress?.({ phase: 'complete', totalFiles: all.size })

	return { project, reachable, all, analysisBoundaries }
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
	if (!config.countTestsAsConsumers && isTestSupportFile(relative)) return true

	for (const pattern of ignoredPaths) {
		if (pattern.includes('**')) {
			if (minimatch(relative, pattern, { dot: true })) {
				return true
			}
		} else {
			if (file === pattern || file.startsWith(pattern + '/')) {
				return true
			}
		}
	}

	if (ignoredFiles.has(relative) || ignoredFiles.has(file)) {
		return true
	}

	for (const pattern of config.ignore.files) {
		if (pattern.includes('**') || pattern.includes('*')) {
			if (minimatch(relative, pattern, { dot: true })) {
				return true
			}
		}
	}

	return false
}

function isTestFile(path: string): boolean {
	return /\.(test|spec)\.[cm]?[jt]sx?$/.test(path)
}

function isTestSupportFile(path: string): boolean {
	return /(^|\/)(__tests__|test|tests)\//.test(path)
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
	all: ReadonlySet<string>
): Readonly<{
	adjacency: ReadonlyMap<string, readonly string[]>
	imported: ReadonlySet<string>
}> {
	const adjacency = new Map<string, readonly string[]>()
	const imported = new Set<string>()
	const resolveModule = createModuleResolver(project, all)

	for (const path of all) {
		const sf = project.getSourceFile(path)
		if (!sf) {
			adjacency.set(path, [])
			continue
		}

		const adjacent = getAdjacentFiles(sf, all, resolveModule)
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
): ReadonlySet<string> {
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
	all: ReadonlySet<string>,
	resolveModule: (fromFile: string, specifier: string) => string | undefined
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
			const resolved = resolveModule(sourceFile.getFilePath(), specifier.getLiteralText!())

			if (resolved) {
				adjacent.add(resolved)
			}

			continue
		}
	}

	return [...adjacent]
}

function createModuleResolver(
	project: Project,
	all: ReadonlySet<string>
): (fromFile: string, specifier: string) => string | undefined {
	const compilerOptions = project.getCompilerOptions()
	const host = ts.createCompilerHost(compilerOptions, true)

	return (fromFile: string, specifier: string) => {
		const resolved = ts.resolveModuleName(
			specifier,
			fromFile,
			compilerOptions,
			host
		).resolvedModule

		if (!resolved) {
			return undefined
		}

		return normalizeResolvedModulePath(resolved.resolvedFileName, all)
	}
}

function normalizeResolvedModulePath(
	resolvedFileName: string,
	all: ReadonlySet<string>
): string | undefined {
	const normalized = resolvedFileName.replaceAll('\\', '/')
	const withoutDts = normalized.replace(/\.d\.ts$/, '')
	const withoutExt = normalized.replace(/\.(mjs|cjs|js|jsx|ts|tsx)$/, '')
	const candidates = [
		normalized,
		`${withoutDts}.ts`,
		`${withoutDts}.tsx`,
		`${withoutExt}.ts`,
		`${withoutExt}.tsx`,
		resolve(withoutExt, 'index.ts'),
		resolve(withoutExt, 'index.tsx'),
	]

	return candidates.find((candidate) => all.has(candidate))
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
