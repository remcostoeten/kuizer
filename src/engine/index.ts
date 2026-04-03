import { Project, SyntaxKind } from 'ts-morph'
import { dirname, isAbsolute, resolve } from 'path'
import fg from 'fast-glob'
import type { AnalysisBoundary, Config } from '../types.js'

/** Initialises the ts-morph project, collects all source files, and runs BFS reachability from resolved entry points. */

export type BootstrapProgress =
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
	})
	await addTestFiles(project, config, root)
	options.onProgress?.({ phase: 'collect-files' })

	const files = project
		.getSourceFiles()
		.map((file) => file.getFilePath())
		.filter((file) => !isIgnored(file, config, root))
	const all = new Set(files)
	options.onProgress?.({ phase: 'resolve-entries', totalFiles: all.size })
	const configuredEntries = await resolveEntries(config, all, root)
	const tests = config.countTestsAsConsumers
		? new Set([...all].filter((file) => isTestFile(file.slice(root.length + 1))))
		: new Set<string>()
	const baseEntries = configuredEntries.size > 0 ? configuredEntries : inferEntries(project, all)
	const entries = new Set([...baseEntries, ...tests])
	const analysisBoundaries: AnalysisBoundary[] = []
	const reachable = bfs(project, all, entries, analysisBoundaries, options.onProgress)
	options.onProgress?.({ phase: 'complete', totalFiles: all.size })

	return { project, reachable, all, analysisBoundaries }
}

function isIgnored(file: string, config: Config, root: string): boolean {
	const relative = file.slice(root.length + 1)
	if (relative.startsWith('node_modules/')) return true
	if (relative.startsWith('.next/')) return true
	if (file.startsWith(config.outputDir + '/')) return true
	if (!config.countTestsAsConsumers && isTestFile(relative)) return true

	for (const path of config.ignore.paths) {
		const absolute = isAbsolute(path) ? path : resolve(root, path)
		if (file === absolute || file.startsWith(absolute + '/')) {
			return true
		}
	}

	return config.ignore.files.includes(relative) || config.ignore.files.includes(file)
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

function inferEntries(project: Project, all: ReadonlySet<string>): ReadonlySet<string> {
	const imported = new Set<string>()

	for (const path of all) {
		const sf = project.getSourceFile(path)
		if (!sf) continue

		for (const next of getAdjacentFiles(sf, all, [])) {
			imported.add(next)
		}
	}

	return new Set([...all].filter((file) => !imported.has(file)))
}

function bfs(
	project: Project,
	all: ReadonlySet<string>,
	entries: ReadonlySet<string>,
	analysisBoundaries: AnalysisBoundary[],
	onProgress?: (event: BootstrapProgress) => void
): ReadonlySet<string> {
	const visited = new Set<string>()
	const queue = [...entries]
	const totalFiles = all.size

	while (queue.length > 0) {
		const path = queue.shift()!
		if (visited.has(path)) continue
		visited.add(path)
		onProgress?.({
			phase: 'analyze-file',
			file: path,
			completed: visited.size,
			totalFiles,
		})

		const sf = project.getSourceFile(path)
		if (!sf) continue

		for (const next of getAdjacentFiles(sf, all, analysisBoundaries)) {
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
	analysisBoundaries: AnalysisBoundary[]
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

	return [...adjacent]
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
