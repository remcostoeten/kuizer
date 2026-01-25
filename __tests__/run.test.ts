import fs from "fs/promises"
import os from "os"
import path from "path"
import { afterEach, beforeEach, describe, expect, test } from "vitest"
import { runJob } from "../src/run.js"
import type { TSet } from "../src/index.js"

function mkTmp() {
    const dir = fs.mkdtemp(path.join(os.tmpdir(), "kuizer-run-"))
    return dir
}

async function mkSet(dir: string): Promise<TSet> {
    const src = path.join(dir, "src")
    await fs.mkdir(src, { recursive: true })

    await fs.writeFile(
        path.join(dir, "tsconfig.json"),
        JSON.stringify(
            {
                compilerOptions: {
                    target: "ES2022",
                    module: "ESNext",
                    moduleResolution: "Bundler",
                    strict: true
                },
                include: ["src/**/*.ts"]
            },
            null,
            2
        )
    )

    return {
        root: dir,
        tsCfg: path.join(dir, "tsconfig.json"),
        src,
        bars: ["index.ts"]
    }
}

describe("runJob", () => {
    let tmpDir: string
    let originalCwd: string

    beforeEach(async () => {
        originalCwd = process.cwd()
        tmpDir = await mkTmp()
        process.chdir(tmpDir)
    })

    afterEach(async () => {
        process.chdir(originalCwd)
        await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {})
    })

    test("returns empty results on empty project", async () => {
        const set = await mkSet(tmpDir)

        const out = await runJob(set, true)

        expect(out.res).toEqual([])
        expect(out.edits).toEqual([])
        expect(out.bak).toBeNull()
    })

    test("dry mode does not create backups", async () => {
        const set = await mkSet(tmpDir)

        const out = await runJob(set, true)

        expect(out.bak).toBeNull()
    })

    test("returns TJobOut structure", async () => {
        const set = await mkSet(tmpDir)

        const out = await runJob(set, true)

        expect(out).toHaveProperty("res")
        expect(out).toHaveProperty("edits")
        expect(out).toHaveProperty("bak")
        expect(Array.isArray(out.res)).toBe(true)
        expect(Array.isArray(out.edits)).toBe(true)
    })

    test("apply mode with no changes does not create backup", async () => {
        const set = await mkSet(tmpDir)

        const out = await runJob(set, false)

        // No edits means no backup needed
        expect(out.bak).toBeNull()
    })
})
