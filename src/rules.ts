export type TMode = "scan" | "fix"

export type TCtx = {
    root: string
    tsCfg: string
    src: string
    exDirs: string[]
}

export type TFind = {
    rule: string
    file: string
    name: string
    line: number
    msg: string
}

export type TEdit = {
    rule: string
    file: string
    next: string
}

export type TRule = {
    id: string
    title: string
    scan: (ctx: TCtx) => TFind[]
    fix?: (ctx: TCtx, finds: TFind[]) => TEdit[]
}
