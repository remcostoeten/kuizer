import { analyze, type TSet, type TRes } from "./index.js"
import { saveBak } from "./backup.js"
import fs from "fs/promises"

export type TEdit = {
    file: string
    prev: string
    next: string
}

export type TJobOut = {
    res: TRes[]
    edits: TEdit[]
    bak: string | null
}

export async function runJob(set: TSet, dry: boolean): Promise<TJobOut> {
    const res = analyze(set)

    const edits: TEdit[] = []
    const byFile = new Map<string, TRes[]>()

    for (const r of res) {
        if (r.act === "keep" || r.act === "keep-local") continue
        const arr = byFile.get(r.file) ?? []
        arr.push(r)
        byFile.set(r.file, arr)
    }

    for (const [file, items] of byFile) {
        const prev = await fs.readFile(file, "utf8").catch(() => null)
        if (prev === null) continue

        const next = applyEdits(prev, items)
        if (next !== prev) {
            edits.push({ file, prev, next })
        }
    }

    let bak: string | null = null

    if (!dry && edits.length > 0) {
        const saved = await saveBak(edits)
        bak = saved.id

        for (const e of edits) {
            await fs.writeFile(e.file, e.next, "utf8")
        }
    }

    return { res, edits, bak }
}

function applyEdits(src: string, items: TRes[]): string {
    const lines = src.split("\n")
    const toRemove = new Set<number>()

    for (const it of items) {
        if (it.act === "rm-decl") {
            const start = it.line - 1
            const end = findDeclEnd(lines, start, it.kind)
            for (let i = start; i <= end; i++) toRemove.add(i)
        } else if (it.act === "unexport") {
            const idx = it.line - 1
            if (idx >= 0 && idx < lines.length) {
                lines[idx] = lines[idx].replace(/^export\s+/, "")
            }
        }
    }

    return lines.filter((_, i) => !toRemove.has(i)).join("\n")
}

function findDeclEnd(lines: string[], start: number, kind: "type" | "iface"): number {
    if (kind === "type") {
        let depth = 0
        let inType = false
        for (let i = start; i < lines.length; i++) {
            const line = lines[i]
            if (!inType && (line.includes("=") || line.includes("{"))) {
                inType = true
            }
            for (const ch of line) {
                if (ch === "{" || ch === "<" || ch === "(") depth++
                if (ch === "}" || ch === ">" || ch === ")") depth--
            }
            if (inType && depth <= 0 && (line.includes(";") || line.trim().endsWith("}"))) {
                return i
            }
        }
    } else {
        let depth = 0
        for (let i = start; i < lines.length; i++) {
            const line = lines[i]
            for (const ch of line) {
                if (ch === "{") depth++
                if (ch === "}") depth--
            }
            if (depth === 0 && line.includes("}")) {
                return i
            }
        }
    }
    return start
}
