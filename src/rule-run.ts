import type { TCtx, TFind, TEdit, TRule } from "./rules"
import { rules } from "./rules-reg"

export type TRunOut = {
    finds: TFind[]
    edits: TEdit[]
}

export function runRules(input: { ctx: TCtx; only?: string[]; skip?: string[] }): TRunOut {
    const only = new Set((input.only ?? []).filter(Boolean))
    const skip = new Set((input.skip ?? []).filter(Boolean))

    const out: TRunOut = { finds: [], edits: [] }

    for (const rule of pickRules(rules(), only, skip)) {
        const finds = rule.scan(input.ctx)
        out.finds.push(...finds)
    }

    return out
}

function pickRules(list: TRule[], only: Set<string>, skip: Set<string>) {
    const out: TRule[] = []
    for (const r of list) {
        if (only.size && !only.has(r.id)) continue
        if (skip.has(r.id)) continue
        out.push(r)
    }
    return out
}
