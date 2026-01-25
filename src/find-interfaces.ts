import path from "path"
import { Project } from "ts-morph"
import { collectFiles } from "./scan"

export type TFindIn = {
    root: string
    tsCfg: string
    src: string
    exDirs?: string[]
}

export type THit = {
    file: string
    name: string
    line: number
}

export function findIface(input: TFindIn): THit[] {
    const ex = new Set((input.exDirs ?? ["ui"]).map(v => v.toLowerCase()))
    const proj = new Project({ tsConfigFilePath: input.tsCfg })

    const ignoreFile = path.join(input.root, "gitignore-example")
    const files = collectFiles({
        root: input.src,
        ignoreFile,
        exts: [".ts", ".tsx"]
    })

    const out: THit[] = []

    for (const abs of files) {
        if (hasSeg(abs, ex)) continue

        const sf = proj.getSourceFile(abs) ?? proj.addSourceFileAtPath(abs)
        const rel = path.relative(input.root, sf.getFilePath())

        for (const node of sf.getInterfaces()) {
            const name = node.getName() ?? ""
            if (!name) continue
            out.push({
                file: rel,
                name,
                line: node.getStartLineNumber()
            })
        }
    }

    out.sort((a, b) => (a.file + a.name).localeCompare(b.file + b.name))
    return out
}

function hasSeg(abs: string, ex: Set<string>) {
    const segs = abs.replaceAll("\\", "/").split("/")
    for (const s of segs) {
        const k = s.toLowerCase()
        if (ex.has(k)) return true
    }
    return false
}
