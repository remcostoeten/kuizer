import fs from "fs"
import path from "path"

export type TScanIn = {
    root: string
    ignoreFile: string
    exts: string[]
}

export function collectFiles(input: TScanIn): string[] {
    const root = path.resolve(input.root)
    const ig = loadIg(input.ignoreFile)

    const out: string[] = []
    walk(root)
    out.sort()
    return out

    function walk(dir: string) {
        const ents = safeRead(dir)
        for (const ent of ents) {
            const abs = path.join(dir, ent.name)
            const rel = path.relative(root, abs)

            if (ent.isDirectory()) {
                if (shouldSkip(rel, ent.name, ig)) continue
                walk(abs)
                continue
            }

            if (!ent.isFile()) continue
            if (!hasExt(ent.name, input.exts)) continue
            if (shouldSkip(rel, ent.name, ig)) continue

            out.push(abs)
        }
    }
}

type TIg = {
    dirs: Set<string>
    files: Set<string>
    pats: string[]
}

function loadIg(file: string): TIg {
    const raw = fs.existsSync(file) ? fs.readFileSync(file, "utf8") : ""
    const lines = raw
        .split("\n")
        .map(l => l.trim())
        .filter(l => l && !l.startsWith("#"))

    const dirs = new Set<string>()
    const files = new Set<string>()
    const pats: string[] = []

    for (const l of lines) {
        const v = norm(l)
        if (!v) continue
        if (v.includes("*")) {
            pats.push(v)
            continue
        }
        if (v.endsWith("/")) {
            dirs.add(v.slice(0, -1))
            continue
        }
        if (v.includes("/")) {
            pats.push(v)
            continue
        }
        dirs.add(v)
        files.add(v)
    }

    dirs.add(".git")
    dirs.add("node_modules")

    return { dirs, files, pats }
}

function shouldSkip(rel: string, base: string, ig: TIg) {
    const r = norm(rel)

    for (const seg of r.split("/")) {
        if (ig.dirs.has(seg)) return true
    }

    if (ig.files.has(base)) return true

    for (const p of ig.pats) {
        if (matchPat(r, p)) return true
    }

    return false
}

function matchPat(rel: string, pat: string) {
    const re = globRe(pat)
    return re.test(rel)
}

function globRe(pat: string) {
    const esc = pat
        .split("")
        .map(c => {
            if (c === "*") return ".*"
            if ("\\.[]{}()+-^$|".includes(c)) return "\\" + c
            return c
        })
        .join("")
    return new RegExp("^" + esc + "$")
}

function safeRead(dir: string) {
    try {
        return fs.readdirSync(dir, { withFileTypes: true })
    } catch {
        return []
    }
}

function hasExt(name: string, exts: string[]) {
    const low = name.toLowerCase()
    for (const e of exts) if (low.endsWith(e)) return true
    return false
}

function norm(v: string) {
    return v
        .replaceAll("\\", "/")
        .replaceAll(/\/+/g, "/")
        .replace(/^\.\//, "")
        .trim()
}
