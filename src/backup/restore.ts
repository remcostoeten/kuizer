import { copyFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs'
import { dirname, isAbsolute, join, relative, resolve } from 'node:path'
import type { RestoreResult, SnapshotManifest } from './types.js'

type RestoreOptions = Readonly<{
	cwd?: string
	outputDir?: string
}>

export function restoreSnapshot(snapshotRef: string, options: RestoreOptions = {}): RestoreResult {
	const cwd = options.cwd ?? process.cwd()
	const resolvedManifestPath = resolveManifestPath(snapshotRef, cwd, options.outputDir)
	const manifest = readSnapshotManifest(resolvedManifestPath)
	const snapshotDir = dirname(resolvedManifestPath)
	const filesDir = join(snapshotDir, 'files')
	const root = manifest.root ?? inferSnapshotRoot(manifest.files, filesDir)

	for (const file of manifest.files) {
		const relativePath = relative(root, file)
		const source = join(filesDir, relativePath)

		if (!existsSync(source)) {
			throw new Error(`Snapshot file is missing for '${file}'`)
		}

		mkdirSync(dirname(file), { recursive: true })
		copyFileSync(source, file)
	}

	return {
		manifestPath: resolvedManifestPath,
		snapshotId: manifest.id,
		restored: manifest.files,
	}
}

function resolveManifestPath(snapshotRef: string, cwd: string, outputDir?: string): string {
	const directPath = resolve(cwd, snapshotRef)
	if (existsSync(directPath)) {
		return directPath
	}

	if (outputDir) {
		const byIdPath = resolve(outputDir, 'snapshots', snapshotRef, 'manifest.json')
		if (existsSync(byIdPath)) {
			return byIdPath
		}
	}

	throw new Error(`Snapshot '${snapshotRef}' was not found`)
}

function readSnapshotManifest(manifestPath: string): SnapshotManifest {
	const raw = JSON.parse(readFileSync(manifestPath, 'utf8')) as Partial<SnapshotManifest>
	if (raw.operation !== 'fix') {
		throw new Error(`Unsupported snapshot operation in '${manifestPath}'`)
	}

	if (!Array.isArray(raw.files) || raw.files.some((file) => typeof file !== 'string')) {
		throw new Error(`Invalid snapshot manifest at '${manifestPath}'`)
	}

	return {
		id: typeof raw.id === 'string' ? raw.id : 'unknown',
		operation: 'fix',
		createdAt: typeof raw.createdAt === 'string' ? raw.createdAt : '',
		...(typeof raw.root === 'string' ? { root: raw.root } : {}),
		files: raw.files,
	}
}

function inferSnapshotRoot(files: readonly string[], filesDir: string): string {
	if (files.length === 0) {
		throw new Error('Snapshot manifest does not contain any files')
	}

	let candidate = dirname(files[0]!)
	while (true) {
		if (isValidSnapshotRoot(candidate, files, filesDir)) {
			return candidate
		}

		const parent = dirname(candidate)
		if (parent === candidate) {
			break
		}

		candidate = parent
	}

	if (isValidSnapshotRoot(candidate, files, filesDir)) {
		return candidate
	}

	throw new Error('Unable to infer snapshot root from manifest')
}

function isValidSnapshotRoot(
	candidate: string,
	files: readonly string[],
	filesDir: string
): boolean {
	return files.every((file) => {
		if (!isAbsolute(file)) {
			return false
		}

		const relativePath = relative(candidate, file)
		if (
			relativePath === '' ||
			relativePath === '..' ||
			relativePath.startsWith('../') ||
			relativePath.startsWith('..\\') ||
			isAbsolute(relativePath)
		) {
			return false
		}

		return existsSync(join(filesDir, relativePath))
	})
}
