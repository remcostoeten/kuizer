import { copyFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs'
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'
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

	const root = resolveSnapshotRoot(manifest.root, snapshotDir)
	const operations = manifest.files.map((file) => resolveSnapshotEntry(root, filesDir, file))

	for (const operation of operations) {
		if (!existsSync(operation.source)) {
			throw new Error(`Snapshot file is missing for '${operation.target}'`)
		}
	}

	for (const operation of operations) {
		mkdirSync(dirname(operation.target), { recursive: true })
		copyFileSync(operation.source, operation.target)
	}

	return {
		manifestPath: resolvedManifestPath,
		snapshotId: manifest.id,
		restored: operations.map((operation) => operation.target),
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

function resolveSnapshotRoot(manifestRoot: string | undefined, snapshotDir: string): string {
	if (!manifestRoot) {
		throw new Error('Snapshot manifest is missing root metadata')
	}

	return isAbsolute(manifestRoot) ? manifestRoot : resolve(snapshotDir, manifestRoot)
}

function resolveSnapshotEntry(
	root: string,
	filesDir: string,
	file: string
): Readonly<{
	target: string
	source: string
}> {
	const target = isAbsolute(file) ? file : resolve(root, file)
	const relativePath = relative(root, target)

	if (
		relativePath === '' ||
		relativePath === '.' ||
		relativePath === '..' ||
		relativePath.startsWith(`..${sep}`) ||
		isAbsolute(relativePath)
	) {
		throw new Error(`Snapshot file '${file}' is outside the snapshot root '${root}'`)
	}

	return {
		target,
		source: join(filesDir, relativePath),
	}
}
