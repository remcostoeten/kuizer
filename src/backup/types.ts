export type SnapshotManifest = Readonly<{
	id: string
	operation: 'fix'
	createdAt: string
	root?: string
	files: readonly string[]
}>

export type SnapshotResult = Readonly<{
	snapshotDir: string
	manifestPath: string
	files: readonly string[]
}>

export type RestoreResult = Readonly<{
	manifestPath: string
	snapshotId: string
	restored: readonly string[]
}>
