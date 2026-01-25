import chalk from "chalk"

export function helpTxt(): string {
    const title = chalk.bold.magentaBright("kuizer")
    const desc = chalk.gray("Deterministic CLI code hygiene tool for TypeScript and JavaScript")
    const section = (s: string) => chalk.bold.cyan(s)
    const flag = (f: string) => chalk.yellow(f)
    const hint = (h: string) => chalk.dim(h)
    const cmd = (c: string) => chalk.green(c)
    const comment = (c: string) => chalk.gray(c)

    return `
${title} ${chalk.dim("—")} ${desc}

${section("USAGE")}
  ${cmd("kuizer")} ${hint("[options]")}

${section("OPTIONS")}
  ${flag("--dry")}         ${hint("Run in dry mode (no changes)")}
  ${flag("--apply")}       ${hint("Apply changes with backup")}
  ${flag("--browse")}      ${hint("Browse latest backups")}
  ${flag("--revert")} ${chalk.white("<id>")} ${hint('Revert to a backup (use "latest" for most recent)')}
  ${flag("--help")}        ${hint("Show this help message")}

${section("INTERACTIVE MODE")}
  ${hint("Run without arguments to enter interactive mode with menu selection.")}

${section("EXAMPLES")}
  ${cmd("kuizer --dry")}      ${comment("# Preview changes")}
  ${cmd("kuizer --apply")}    ${comment("# Apply changes")}
  ${cmd("kuizer --browse")}   ${comment("# List backups")}
  ${cmd("kuizer --revert latest")}  ${comment("# Revert last change")}
`.trim()
}
