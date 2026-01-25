export type TScanIn = {
    root: string
    ignoreFile: string
    exts: string[]
}

export function collectFiles(_: TScanIn): string[] {
    return []
}
