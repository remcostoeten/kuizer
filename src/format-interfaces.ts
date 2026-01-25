import path from "path"
import type { THit } from "./find-interfaces"

type TGrp = {
    dir: string
    qty: number
    files: Map<string, THit[]>
}

export function fmtIface(hits: THit[]) {
    const total = hits.length
    const byDir = grpDir(hits)

    const lines: string[] = []
    lines.push(`interfaces: ${total}`)

    for (const g of byDir) {
        lines.push("")
        lines.push(`${g.dir}  (${g.qty})`)

        const files = [...g.files.entries()].sort((a, b) =>
            a[0].localeCompare(b[0])
        )
        for (const [file, list] of files) {
            lines.push(`  ${file}  (${list.length})`)
            for (const h of list.sort((a, b) => a.line - b.line)) {
                lines.push(`    ${h.name}  line ${h.line}`)
            }
        }
    }

    lines.push("")
    return lines.join("\n")
}

function grpDir(hits: THit[]) {
    const map = new Map<string, TGrp>()

    for (const h of hits) {
        const dir = normDir(h.file)
        if (!map.has(dir)) map.set(dir, { dir, qty: 0, files: new Map() })

        const g = map.get(dir)!
        g.qty++

        const file = h.file
        if (!g.files.has(file)) g.files.set(file, [])
        g.files.get(file)!.push(h)
    }

    return [...map.values()].sort((a, b) => a.dir.localeCompare(b.dir))
}

function normDir(file: string) {
    const d = path.posix.dirname(file.replaceAll("\\", "/"))
    return d === "." ? "root" : d
}
