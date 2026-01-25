import fs from "fs"
import os from "os"
import path from "path"
import { describe, expect, test } from "vitest"
import { findIface } from "../src/find-interfaces.js"

function mkTmp() {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "kuizer-find-"))
    const src = path.join(dir, "src")
    fs.mkdirSync(src, { recursive: true })

    fs.writeFileSync(
        path.join(dir, "tsconfig.json"),
        JSON.stringify(
            {
                compilerOptions: {
                    target: "ES2022",
                    module: "ESNext",
                    moduleResolution: "Bundler",
                    strict: true
                },
                include: ["src/**/*.ts", "src/**/*.tsx"]
            },
            null,
            2
        )
    )

    return { dir, src }
}

describe("findIface", () => {
    test("finds interface declarations in ts files", () => {
        const { dir, src } = mkTmp()

        fs.writeFileSync(
            path.join(src, "types.ts"),
            `interface User {
    name: string
    age: number
}

interface Post {
    title: string
}`
        )

        const hits = findIface({
            root: dir,
            tsCfg: path.join(dir, "tsconfig.json"),
            src
        })

        expect(hits).toHaveLength(2)
        expect(hits.map(h => h.name).sort()).toEqual(["Post", "User"])
    })

    test("finds interfaces in tsx files", () => {
        const { dir, src } = mkTmp()

        fs.writeFileSync(
            path.join(src, "Component.tsx"),
            `interface Props {
    label: string
}

export function Button(p: Props) {
    return <button>{p.label}</button>
}`
        )

        const hits = findIface({
            root: dir,
            tsCfg: path.join(dir, "tsconfig.json"),
            src
        })

        expect(hits).toHaveLength(1)
        expect(hits[0].name).toBe("Props")
    })

    test("returns line numbers for interfaces", () => {
        const { dir, src } = mkTmp()

        fs.writeFileSync(
            path.join(src, "types.ts"),
            `// comment
// another comment

interface First {
    a: number
}

interface Second {
    b: string
}`
        )

        const hits = findIface({
            root: dir,
            tsCfg: path.join(dir, "tsconfig.json"),
            src
        })

        const first = hits.find(h => h.name === "First")
        const second = hits.find(h => h.name === "Second")

        expect(first?.line).toBe(4)
        expect(second?.line).toBe(8)
    })

    test("excludes directories specified in exDirs", () => {
        const { dir, src } = mkTmp()

        fs.mkdirSync(path.join(src, "ui"), { recursive: true })

        fs.writeFileSync(
            path.join(src, "types.ts"),
            `interface Keep { a: number }`
        )

        fs.writeFileSync(
            path.join(src, "ui", "types.ts"),
            `interface Exclude { b: number }`
        )

        const hits = findIface({
            root: dir,
            tsCfg: path.join(dir, "tsconfig.json"),
            src,
            exDirs: ["ui"]
        })

        expect(hits).toHaveLength(1)
        expect(hits[0].name).toBe("Keep")
    })

    test("returns empty array when no interfaces found", () => {
        const { dir, src } = mkTmp()

        fs.writeFileSync(
            path.join(src, "types.ts"),
            `type User = { name: string }`
        )

        const hits = findIface({
            root: dir,
            tsCfg: path.join(dir, "tsconfig.json"),
            src
        })

        expect(hits).toHaveLength(0)
    })

    test("finds interfaces in nested directories", () => {
        const { dir, src } = mkTmp()

        fs.mkdirSync(path.join(src, "features", "auth"), { recursive: true })

        fs.writeFileSync(
            path.join(src, "features", "auth", "types.ts"),
            `interface AuthState {
    isLoggedIn: boolean
}`
        )

        const hits = findIface({
            root: dir,
            tsCfg: path.join(dir, "tsconfig.json"),
            src
        })

        expect(hits).toHaveLength(1)
        expect(hits[0].name).toBe("AuthState")
    })
})
