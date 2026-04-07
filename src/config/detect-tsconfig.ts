import { stat } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'

async function pathExists(path: string): Promise<boolean> {
	try {
		await stat(path)
		return true
	} catch {
		return false
	}
}

async function findNearestTsConfig(startDir: string): Promise<string | null> {
	let dir = resolve(startDir)

	while (true) {
		const candidate = join(dir, 'tsconfig.json')
		if (await pathExists(candidate)) {
			return candidate
		}

		const parent = dirname(dir)
		if (parent === dir) {
			return null
		}

		dir = parent
	}
}

/** Resolves an explicit tsconfig path or finds the nearest tsconfig.json upward from cwd. */
export async function detectTsConfig(cwd: string, explicitPath?: string): Promise<string | null> {
	if (explicitPath) {
		return resolve(cwd, explicitPath)
	}

	return findNearestTsConfig(cwd)
}
