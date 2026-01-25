import path from "path"
import chalk from "chalk"
import { select } from "@inquirer/prompts"
import { readArg } from "./args"
import { helpTxt } from "./help"
import { lastId, listBak, revBak } from "./backup"
import { runJob } from "./run"
import { runRules } from "./rule-run"
import { fmtIface } from "./format-interfaces"
import type { TSet } from "./index"

type TOpt = "dry" | "apply" | "browse" | "revert" | "iface" | "help" | "exit"

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
    const arg = readArg(process.argv.slice(2))

    if (hasHelp(process.argv.slice(2))) {
        console.log(helpTxt())
        return
    }

    if (arg.listIface) {
        await doIface()
        return
    }

    if (arg.browse) {
        await doBrowse()
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
                { name: "List interfaces (exclude ui)", value: "iface" },
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
        if (opt === "iface") {
            await doIface()
            continue
        }
        if (opt === "browse") {
            await doBrowse()
            continue
        }
        if (opt === "revert") {
            await doRev("latest")
            continue
        }
        if (opt === "dry") {
            await doRun(true)
            continue
        }
        if (opt === "apply") {
            await doRun(false)
            continue
        }
    }
}

function hasHelp(argv: string[]) {
    for (const a of argv) if (a === "--help" || a === "-h") return true
    return false
}

async function doIface() {
    const set = pickSet()
    const out = runRules({
        ctx: {
            root: set.root,
            tsCfg: set.tsCfg,
            src: set.src,
            exDirs: ["ui"]
        },
        only: ["list-interfaces"]
    })

    const hits = out.finds.map(f => ({
        file: f.file,
        name: f.name,
        line: f.line
    }))
    console.log(fmtIface(hits))
}

async function doRun(dry: boolean) {
    const set = pickSet()
    const out = await runJob(set, dry)

    const line = chalk.gray(
        "------------------------------------------------------------"
    )
    const key = (v: string) => chalk.cyan(v)
    const val = (v: string) => chalk.white(v)

    console.log(line)
    console.log(chalk.bold.gray("Run"))
    console.log(`  ${key("mode")}   ${val(dry ? "dry" : "apply")}`)
    console.log(`  ${key("hits")}   ${val(String(out.res.length))}`)
    console.log(`  ${key("edits")}  ${val(String(out.edits.length))}`)
    if (out.bak) console.log(`  ${key("bak")}   ${val(out.bak)}`)

    if (out.edits.length) {
        console.log(line)
        console.log(chalk.bold.gray("Files"))
        printEd(out.edits.map(x => ({ file: x.file })))
    }

    console.log(line)

    if (dry && out.edits.length > 0) {
        process.exitCode = 1
    }
}

async function doBrowse() {
    const list = await listBak()
    if (list.length === 0) {
        console.log(chalk.yellow("no backups"))
        return
    }

    const line = chalk.gray(
        "------------------------------------------------------------"
    )
    console.log(line)
    console.log(chalk.bold.gray("Backups"))

    for (const it of list) {
        const id = chalk.cyan(it.id)
        const ts = chalk.dim(it.meta.ts)
        const n = chalk.white(String(it.meta.files.length))
        console.log(`  ${id}  ${ts}  files=${n}`)
    }

    console.log(line)
}

async function doRev(id: string) {
    const useId = id === "latest" ? await lastId() : id
    if (!useId) {
        console.log(chalk.yellow("no backups"))
        return
    }

    const res = await revBak(useId)
    if (!res.ok) {
        console.log(chalk.red(res.err))
        process.exitCode = 2
        return
    }

    console.log(chalk.green(`reverted: ${res.id}`))
}

function printEd(edits: Array<{ file: string }>) {
    const rel = edits.map(e => path.relative(process.cwd(), e.file))
    for (const p of rel.slice(0, 50)) console.log(`  ${chalk.dim(p)}`)
    if (rel.length > 50)
        console.log(`  ${chalk.dim(`... +${rel.length - 50}`)}`)
}
