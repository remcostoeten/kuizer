import { Project } from 'ts-morph'
import type { Ctx, Config } from '../../src/types.js'
import { defaults } from '../../src/config/defaults.js'

type Files = Record<string, string>

/** 
 * @name makeCtx, makeConfig 
 * 
 * * @description Helpers to create a Ctx and Config for testing detectors. The Ctx is created using ts-morph's in-memory file system, so you can provide a simple object mapping file paths to their contents. The Config can be overridden with any partial config values, and will be merged with the defaults. 
 */

export function makeCtx(all: Files, reachable: Files): Ctx {
    const project = new Project({ useInMemoryFileSystem: true })

    for (const [path, content] of Object.entries(all)) {
        project.createSourceFile(path, content)
    }

    return {
        project,
        all: new Set(Object.keys(all)),
        reachable: new Set(Object.keys(reachable)),
        analysisBoundaries: [],
    }
}

export function makeConfig(overrides: Partial<Config> = {}): Config {
    return { ...defaults, ...overrides }
}
