import { randomUUID } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'
import type { Config } from '../types.js'
import type { PlannedEdit } from '../fixers/types.js'
import type { SnapshotManifest, SnapshotResult } from './types.js'

export function createSnapshot(edits: readonly PlannedEdit[], config: Config): SnapshotResult {
	const files = [...new Set(edits.map((edit) => edit.file))].sort()
	const createdAt = new Date().toISOString()
	const snapshotId = `${createdAt.replace(/[:.]/g, '-')}-fix-${randomUUID().slice(0, 8)}`
	const root = resolve(dirname(config.tsconfig))
	const snapshotDir = join(config.outputDir, 'snapshots', snapshotId)
	const filesDir = join(snapshotDir, 'files')
	const entries = files.map((file) => ({
		file,
		relativePath: getSnapshotRelativePath(root, file),
	}))

	mkdirSync(filesDir, { recursive: true })

	for (const entry of entries) {
		const target = join(filesDir, entry.relativePath)
		mkdirSync(dirname(target), { recursive: true })
		writeFileSync(target, readFileSync(entry.file, 'utf8'))
	}

	const manifest: SnapshotManifest = {
		id: snapshotId,
		operation: 'fix',
		createdAt,
		root,
		files,
	}

	const manifestPath = join(snapshotDir, 'manifest.json')
	writeFileSync(manifestPath, JSON.stringify(manifest, null, 2))

	return {
		snapshotDir,
		manifestPath,
		files,
	}
}

function getSnapshotRelativePath(root: string, file: string): string {
	const absoluteRoot = resolve(root)
	const absoluteFile = resolve(file)
	const relativePath = relative(absoluteRoot, absoluteFile)

	if (
		relativePath === '' ||
		relativePath === '.' ||
		relativePath === '..' ||
		relativePath.startsWith(`..${sep}`) ||
		isAbsolute(relativePath)
	) {
		throw new Error(`Snapshot file '${file}' is outside the snapshot root '${absoluteRoot}'`)
	}

	return relativePath
}
