import chalk from "chalk"

export type TUi = {
    h1: (v: string) => string
    h2: (v: string) => string
    dim: (v: string) => string
    ok: (v: string) => string
    warn: (v: string) => string
    err: (v: string) => string
    key: (v: string) => string
    val: (v: string) => string
    line: () => string
}

export function ui(): TUi {
    return {
        h1(v) {
            return chalk.bold.white(v)
        },
        h2(v) {
            return chalk.bold.gray(v)
        },
        dim(v) {
            return chalk.gray(v)
        },
        ok(v) {
            return chalk.green(v)
        },
        warn(v) {
            return chalk.yellow(v)
        },
        err(v) {
            return chalk.red(v)
        },
        key(v) {
            return chalk.cyan(v)
        },
        val(v) {
            return chalk.white(v)
        },
        line() {
            return chalk.gray(
                "------------------------------------------------------------"
            )
        }
    }
}
