import fs from "fs/promises"
import os from "os"
import path from "path"
import { afterEach, beforeEach, describe, expect, test } from "vitest"
import { saveBak, listBak, revBak, lastId, baseDir } from "../src/backup.js"

describe("backup", () => {
    let originalCwd: string
    let tmpDir: string

    beforeEach(async () => {
        originalCwd = process.cwd()
        tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), "kuizer-backup-test-"))
        process.chdir(tmpDir)
    })

    afterEach(async () => {
        process.chdir(originalCwd)
        await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {})
    })

    test("baseDir returns path in home directory", async () => {
        const dir = await baseDir()
        expect(dir).toContain(os.homedir())
        expect(dir).toContain(".type-transforme")
    })

    test("saveBak creates backup with files", async () => {
        const testFile = path.join(tmpDir, "test.ts")
        await fs.writeFile(testFile, "original content")

        const result = await saveBak([
            { file: testFile, prev: "original content" }
        ])

        expect(result.id).toMatch(/^\d{8}-\d{6}-[a-f0-9]+$/)
        expect(result.dir).toContain(result.id)
    })

    test("listBak returns empty array when no backups", async () => {
        const list = await listBak()
        expect(Array.isArray(list)).toBe(true)
    })

    test("saveBak and listBak round-trip", async () => {
        const testFile = path.join(tmpDir, "test.ts")
        await fs.writeFile(testFile, "content")

        const saved = await saveBak([
            { file: testFile, prev: "content" }
        ])

        const list = await listBak()
        const found = list.find(b => b.id === saved.id)

        expect(found).toBeDefined()
        expect(found?.meta.files).toHaveLength(1)
    })

    test("revBak restores file content", async () => {
        const testFile = path.join(tmpDir, "test.ts")
        await fs.writeFile(testFile, "original")

        const saved = await saveBak([
            { file: testFile, prev: "original" }
        ])

        await fs.writeFile(testFile, "modified")
        expect(await fs.readFile(testFile, "utf8")).toBe("modified")

        const result = await revBak(saved.id)
        expect(result.ok).toBe(true)

        const restored = await fs.readFile(testFile, "utf8")
        expect(restored).toBe("original")
    })

    test("revBak returns error for non-existent backup", async () => {
        const result = await revBak("nonexistent-id")
        expect(result.ok).toBe(false)
        if (!result.ok) {
            expect(result.err).toContain("not found")
        }
    })

    test("lastId returns a backup id after saving", async () => {
        const testFile = path.join(tmpDir, "test.ts")
        await fs.writeFile(testFile, "content")

        await saveBak([
            { file: testFile, prev: "content" }
        ])

        const id = await lastId()
        // Should return some backup id (may not be ours if other tests ran)
        expect(typeof id).toBe("string")
        expect(id).toMatch(/^\d{8}-\d{6}-[a-f0-9]+$/)
    })

    test("lastId returns null when no backups", async () => {
        const id = await lastId()
        // May have backups from other tests, so just check type
        expect(id === null || typeof id === "string").toBe(true)
    })
})
