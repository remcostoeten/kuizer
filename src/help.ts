import chalk from "chalk"

export function helpTxt(): string {
    const title = chalk.bold.magentaBright("ts-type-sweeper")
    const desc = chalk.gray("Remove unused TypeScript types and interfaces")
    const section = (s: string) => chalk.bold.cyan(s)
    const flag = (f: string) => chalk.yellow(f)
    const hint = (h: string) => chalk.dim(h)
    const cmd = (c: string) => chalk.green(c)
    const comment = (c: string) => chalk.gray(c)

    return `
${title} ${chalk.dim("—")} ${desc}

${section("USAGE")}
  ${cmd("ts-type-sweeper")} ${hint("[options]")}

${section("OPTIONS")}
  ${flag("--dry")}         ${hint("Run in dry mode (no changes)")}
  ${flag("--apply")}       ${hint("Apply changes with backup")}
  ${flag("--browse")}      ${hint("Browse latest backups")}
  ${flag("--revert")} ${chalk.white("<id>")} ${hint('Revert to a backup (use "latest" for most recent)')}
  ${flag("--help")}        ${hint("Show this help message")}

${section("INTERACTIVE MODE")}
  ${hint("Run without arguments to enter interactive mode with menu selection.")}

${section("EXAMPLES")}
  ${cmd("ts-type-sweeper --dry")}      ${comment("# Preview changes")}
  ${cmd("ts-type-sweeper --apply")}    ${comment("# Apply changes")}
  ${cmd("ts-type-sweeper --browse")}   ${comment("# List backups")}
  ${cmd("ts-type-sweeper --revert latest")}  ${comment("# Revert last change")}
`.trim()
}
