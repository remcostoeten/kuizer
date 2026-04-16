import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import type { AnalysisBoundary } from '../types.js'

export type CachedContext = {
	adjacent?: string[] | undefined
	boundaries?: AnalysisBoundary[] | undefined
	consumerKeys?: string[] | undefined
}

type FileCacheEntry = {
	hash: string
	context: CachedContext
}

type CacheManifest = {
	version: string
	files: Record<string, FileCacheEntry>
}

const CACHE_VERSION = 'v1'

export class FileCacheManager {
	private readonly cachePath: string
	private readonly root: string
	private manifest: CacheManifest
	private isDirty = false
	private fileHashes = new Map<string, string>()

	constructor(cwd: string) {
		this.root = cwd
		this.cachePath = resolve(cwd, 'node_modules', '.cache', 'kuizer', 'analyze-cache.json')
		this.manifest = this.loadManifest()
	}

	private loadManifest(): CacheManifest {
		try {
			if (existsSync(this.cachePath)) {
				const data = JSON.parse(readFileSync(this.cachePath, 'utf8')) as CacheManifest
				if (data.version === CACHE_VERSION && typeof data.files === 'object') {
					return data
				}
			}
		} catch (e) {
			// ignore and return new
		}
		return { version: CACHE_VERSION, files: {} }
	}

	public getFileHash(absolutePath: string): string | undefined {
		const existing = this.fileHashes.get(absolutePath)
		if (existing) return existing

		try {
			if (!existsSync(absolutePath)) return undefined
			const stat = statSync(absolutePath)
			// fast hash: size + mtimeMs. If precision is identical, we assume unchanged.
			const sizePart = stat.size.toString(16)
			const timePart = stat.mtimeMs.toString(16)
			const hash = `${sizePart}-${timePart}`
			this.fileHashes.set(absolutePath, hash)
			return hash
		} catch (e) {
			return undefined
		}
	}

	public getCachedFileContext(absolutePath: string): CachedContext | undefined {
		const hash = this.getFileHash(absolutePath)
		if (!hash) return undefined

		const entry = this.manifest.files[absolutePath]
		if (entry && entry.hash === hash) {
			return entry.context
		}
		return undefined
	}

	public setCachedFileContext(absolutePath: string, partialCtx: Partial<CachedContext>): void {
		const hash = this.getFileHash(absolutePath)
		if (!hash) return

		const current = this.manifest.files[absolutePath]
		this.manifest.files[absolutePath] = {
			hash,
			context: {
				adjacent: partialCtx.adjacent ?? current?.context?.adjacent,
				boundaries: partialCtx.boundaries ?? current?.context?.boundaries,
				consumerKeys: partialCtx.consumerKeys ?? current?.context?.consumerKeys,
			},
		}
		this.isDirty = true
	}

	public save(): void {
		if (!this.isDirty) return

		try {
			const dir = dirname(this.cachePath)
			if (!existsSync(dir)) {
				mkdirSync(dir, { recursive: true })
			}

			// Clean up removed files
			for (const file of Object.keys(this.manifest.files)) {
				if (!existsSync(file)) {
					delete this.manifest.files[file]
				}
			}

			writeFileSync(this.cachePath, JSON.stringify(this.manifest, null, 2), 'utf8')
			this.isDirty = false
		} catch (e) {
			// silence cache write errors 
		}
	}
}
