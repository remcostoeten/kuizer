import { randomUUID } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import type { Config } from '../types.js'
import type { PlannedEdit } from '../fixers/types.js'
import type { SnapshotManifest, SnapshotResult } from './types.js'

export function createSnapshot(edits: readonly PlannedEdit[], config: Config): SnapshotResult {
	const files = [...new Set(edits.map((edit) => edit.file))].sort()
	const createdAt = new Date().toISOString()
	const snapshotId = `${createdAt.replace(/[:.]/g, '-')}-fix-${randomUUID().slice(0, 8)}`
	const root = dirname(config.tsconfig)
	const snapshotDir = join(config.outputDir, 'snapshots', snapshotId)
	const filesDir = join(snapshotDir, 'files')

	mkdirSync(filesDir, { recursive: true })

	for (const file of files) {
		const target = join(filesDir, relative(root, file))
		mkdirSync(dirname(target), { recursive: true })
		writeFileSync(target, readFileSync(file, 'utf8'))
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
