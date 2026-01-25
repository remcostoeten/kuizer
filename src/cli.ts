import path from "path"
import { select } from "@inquirer/prompts"
import { readArg } from "./args.js"
import { lastId, listBak, revBak } from "./backup.js"
import { runJob } from "./run.js"
import type { TSet } from "./index.js"
import { helpTxt } from "./help.js"
import { ui } from "./ui.js"

type TOpt = "dry" | "apply" | "browse" | "revert" | "help" | "exit"

function pickSet(): TSet {
    const root = process.cwd()
    return {
        root,
        tsCfg: path.join(root, "tsconfig.json"),
        src: root,
        bars: ["index.ts", "index.tsx"]
    }
}

export async function main() {
    const u = ui()
    const arg = readArg(process.argv.slice(2))

    if (arg.browse) {
        await showBak()
        return
    }

    if (arg.revert) {
        await doRev(arg.revert)
        return
    }

    if (arg.apply) {
        await doRun(false)
        return
    }

    if (arg.dry && process.argv.length > 2) {
        await doRun(true)
        return
    }

    for (;;) {
        const opt = await select<TOpt>({
            message: "Select an action",
            choices: [
                { name: "Dry run", value: "dry" },
                { name: "Apply with backup", value: "apply" },
                { name: "Browse latest 5 backups", value: "browse" },
                { name: "Revert latest backup", value: "revert" },
                { name: "Help", value: "help" },
                { name: "Exit", value: "exit" }
            ]
        })

        if (opt === "exit") return
        if (opt === "help") {
            console.log(helpTxt())
            continue
        }
        if (opt === "dry") await doRun(true)
        if (opt === "apply") await doRun(false)
        if (opt === "browse") await showBak()
        if (opt === "revert") await doRev("latest")
    }

    async function doRun(dry: boolean) {
        const set = pickSet()
        const out = await runJob(set, dry)

        console.log(u.line())
        console.log(u.h2("Run"))
        console.log(`  ${u.key("mode")}   ${u.val(dry ? "dry" : "apply")}`)
        console.log(`  ${u.key("hits")}   ${u.val(String(out.res.length))}`)
        console.log(`  ${u.key("edits")}  ${u.val(String(out.edits.length))}`)

        if (out.bak) console.log(`  ${u.key("bak")}   ${u.val(out.bak)}`)

        if (out.edits.length) {
            console.log(u.line())
            console.log(u.h2("Files"))
            printEd(out.edits.map((x: { file: string }) => ({ file: x.file })))
        }

        console.log(u.line())
    }

    async function showBak() {
        const list = await listBak()
        if (list.length === 0) {
            console.log(u.warn("no backups"))
            return
        }

        console.log(u.line())
        console.log(u.h2("Backups"))

        for (const it of list) {
            const id = u.key(it.id)
            const ts = u.dim(it.meta.ts)
            const n = u.val(String(it.meta.files.length))
            console.log(`  ${id}  ${ts}  files=${n}`)
        }

        console.log(u.line())
    }

    async function doRev(id: string) {
        const useId = id === "latest" ? await lastId() : id
        if (!useId) {
            console.log(u.warn("no backups"))
            return
        }

        const res = await revBak(useId)
        if (!res.ok) {
            console.log(u.err(res.err))
            return
        }

        console.log(u.ok(`reverted: ${res.id}`))
    }

    function printEd(edits: Array<{ file: string }>) {
        const rel = edits.map(e => path.relative(process.cwd(), e.file))
        for (const p of rel.slice(0, 50)) console.log(`  ${u.dim(p)}`)
        if (rel.length > 50)
            console.log(`  ${u.dim(`... +${rel.length - 50}`)}`)
    }
}
