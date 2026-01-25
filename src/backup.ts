// src/backup.ts
import fs from "fs/promises"
import path from "path"
import os from "os"

type TEnt = {
    id: string
    ts: string
    files: Array<{ file: string }>
}

type TSave = {
    id: string
    dir: string
}

export async function baseDir() {
    const home = os.homedir()
    return path.join(home, ".type-transforme")
}

export async function listBak() {
    const dir = await baseDir()
    await fs.mkdir(dir, { recursive: true })

    const names = await safeRead(dir)
    const ids = names
        .filter(n => n.endsWith(".json"))
        .map(n => n.replace(/\.json$/, ""))
    ids.sort((a, b) => b.localeCompare(a))

    const out: Array<{ id: string; meta: TEnt }> = []
    for (const id of ids.slice(0, 50)) {
        const p = path.join(dir, `${id}.json`)
        const raw = await fs.readFile(p, "utf8").catch(() => "")
        if (!raw) continue
        const meta = parseMeta(raw)
        if (!meta) continue
        out.push({ id, meta })
    }

    return out.slice(0, 5)
}

export async function saveBak(files: Array<{ file: string; prev: string }>) {
    const dir = await baseDir()
    await fs.mkdir(dir, { recursive: true })

    const id = mkId()
    const bak = path.join(dir, id)
    await fs.mkdir(bak, { recursive: true })

    for (const f of files) {
        const p = path.join(bak, relKey(f.file))
        await fs.mkdir(path.dirname(p), { recursive: true })
        await fs.writeFile(p, f.prev, "utf8")
    }

    const meta: TEnt = {
        id,
        ts: new Date().toISOString(),
        files: files.map(f => ({ file: f.file }))
    }

    await fs.writeFile(
        path.join(dir, `${id}.json`),
        JSON.stringify(meta, null, 2),
        "utf8"
    )
    await pruneBak()

    return { id, dir: bak } satisfies TSave
}

export async function revBak(id: string) {
    const dir = await baseDir()
    const meta = await readMeta(id)
    if (!meta) return { ok: false as const, err: `backup not found: ${id}` }

    const bak = path.join(dir, id)
    for (const it of meta.files) {
        const p = path.join(bak, relKey(it.file))
        const raw = await fs.readFile(p, "utf8").catch(() => null)
        if (raw === null) continue
        await fs.mkdir(path.dirname(it.file), { recursive: true })
        await fs.writeFile(it.file, raw, "utf8")
    }

    return { ok: true as const, id }
}

export async function lastId() {
    const list = await listBak()
    return list[0]?.id ?? null
}

async function pruneBak() {
    const dir = await baseDir()
    const list = await listBak()
    const keep = new Set(list.map(x => x.id))

    const names = await safeRead(dir)
    for (const n of names) {
        if (n === "." || n === "..") continue

        if (n.endsWith(".json")) {
            const id = n.replace(/\.json$/, "")
            if (keep.has(id)) continue
            await fs.unlink(path.join(dir, n)).catch(() => void 0)
            continue
        }

        const p = path.join(dir, n)
        const st = await fs.stat(p).catch(() => null)
        if (!st) continue
        if (!st.isDirectory()) continue
        if (keep.has(n)) continue

        await fs.rm(p, { recursive: true, force: true }).catch(() => void 0)
    }
}

function mkId() {
    const d = new Date()
    const a = pad(d.getFullYear(), 4)
    const b = pad(d.getMonth() + 1, 2)
    const c = pad(d.getDate(), 2)
    const e = pad(d.getHours(), 2)
    const f = pad(d.getMinutes(), 2)
    const g = pad(d.getSeconds(), 2)
    const r = Math.random().toString(16).slice(2, 10)
    return `${a}${b}${c}-${e}${f}${g}-${r}`
}

function pad(n: number, len: number) {
    const s = String(n)
    if (s.length >= len) return s
    return "0".repeat(len - s.length) + s
}

function relKey(abs: string) {
    const root = process.cwd()
    const rel = path.relative(root, abs)
    return rel.startsWith("..") ? abs.replace(/[/:\\]/g, "_abs_") : rel
}

async function safeRead(dir: string) {
    return await fs.readdir(dir).catch(() => [])
}

function parseMeta(raw: string) {
    try {
        const v = JSON.parse(raw) as TEnt
        if (!v?.id || !Array.isArray(v.files)) return null
        return v
    } catch {
        return null
    }
}

async function readMeta(id: string) {
    const dir = await baseDir()
    const p = path.join(dir, `${id}.json`)
    const raw = await fs.readFile(p, "utf8").catch(() => "")
    if (!raw) return null
    return parseMeta(raw)
}
