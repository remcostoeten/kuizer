type TArg = {
    dry: boolean
    apply: boolean
    revert: string | null
    browse: boolean
}

export function readArg(argv: string[]): TArg {
    const out: TArg = { dry: true, apply: false, revert: null, browse: false }

    for (let i = 0; i < argv.length; i++) {
        const a = argv[i] ?? ""

        if (a === "--apply") {
            out.apply = true
            out.dry = false
            continue
        }

        if (a === "--dry") {
            out.apply = false
            out.dry = true
            continue
        }

        if (a === "--browse") {
            out.browse = true
            continue
        }

        if (a === "--revert") {
            const v = argv[i + 1] ?? ""
            out.revert = v && !v.startsWith("--") ? v : "latest"
            if (v && !v.startsWith("--")) i++
            continue
        }

        if (a.startsWith("--revert=")) {
            const v = a.slice("--revert=".length)
            out.revert = v || "latest"
            continue
        }
    }

    return out
}
