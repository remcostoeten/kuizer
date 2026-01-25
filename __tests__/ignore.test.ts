import fs from "fs"
import os from "os"
import path from "path"
import { describe, expect, test } from "vitest"
import { collectFiles } from "../src/scan.js"

function mkTmp() {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "scan-ignore-"))
    return dir
}

function write(p: string, c = "") {
    fs.mkdirSync(path.dirname(p), { recursive: true })
    fs.writeFileSync(p, c)
}

describe("recursive ignore rules", () => {
    test("ignores files inside ignored folders", () => {
        const root = mkTmp()

        write(path.join(root, "src/a.ts"), "type A = {}")
        write(path.join(root, "node_modules/x.ts"), "type X = {}")
        write(path.join(root, "dist/y.ts"), "type Y = {}")

        write(
            path.join(root, "gitignore-example"),
            ["node_modules", "dist"].join("\n")
        )

        const files = collectFiles({
            root,
            ignoreFile: path.join(root, "gitignore-example"),
            exts: [".ts"]
        })

        expect(files.map(f => path.relative(root, f))).toEqual(["src/a.ts"])
    })

    test("same filename outside ignored folder is still scanned", () => {
        const root = mkTmp()

        write(path.join(root, "src/index.ts"), "type A = {}")
        write(path.join(root, "dist/index.ts"), "type B = {}")

        write(path.join(root, "gitignore-example"), "dist")

        const files = collectFiles({
            root,
            ignoreFile: path.join(root, "gitignore-example"),
            exts: [".ts"]
        })

        expect(files.map(f => path.relative(root, f))).toEqual(["src/index.ts"])
    })

    test("nested ignored folders are skipped recursively", () => {
        const root = mkTmp()

        write(path.join(root, "apps/web/src/a.ts"), "type A = {}")
        write(path.join(root, "apps/web/.next/b.ts"), "type B = {}")

        write(path.join(root, "gitignore-example"), ".next")

        const files = collectFiles({
            root,
            ignoreFile: path.join(root, "gitignore-example"),
            exts: [".ts"]
        })

        expect(files.map(f => path.relative(root, f))).toEqual([
            "apps/web/src/a.ts"
        ])
    })
})
