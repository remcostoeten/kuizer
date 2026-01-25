import { findIface } from "./find-interfaces"
import type { TRule, TCtx, TFind } from "./rules"

export function rules(): TRule[] {
    return [ifaceRule()]
}

function ifaceRule(): TRule {
    return {
        id: "list-interfaces",
        title: "List interfaces (exclude ui)",
        scan(ctx: TCtx): TFind[] {
            const hits = findIface({
                root: ctx.root,
                tsCfg: ctx.tsCfg,
                src: ctx.src,
                exDirs: ctx.exDirs,
            })

            return hits.map((h) => ({
                rule: "list-interfaces",
                file: h.file,
                name: h.name,
                line: h.line,
                msg: "interface declaration",
            }))
        },
    }
}
