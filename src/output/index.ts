import pc from "picocolors";

export type Severity = "error" | "warn" | "info";

export function dim(s: string) {
  return pc.dim(s);
}

export function bold(s: string) {
  return pc.bold(s);
}

export function colorize(s: string, severity: Severity) {
  if (severity === "error") return pc.red(s);
  if (severity === "warn") return pc.yellow(s);
  return pc.cyan(s);
}

export function badge(severity: Severity) {
  const label = severity.padEnd(5);
  return colorize(label, severity);
}

export function rule(width = 60) {
  return dim("─".repeat(width));
}

export function indent(s: string, n = 2) {
  const pad = " ".repeat(n);
  return s
    .split("\n")
    .map((line) => pad + line)
    .join("\n");
}

export function write(s: string) {
  process.stdout.write(s + "\n");
}

export function spinner(msg: string) {
  const frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
  let i = 0;
  const id = setInterval(() => {
    process.stdout.write(`\r${pc.cyan(frames[i++ % frames.length]!)} ${msg}`);
  }, 80);
  return () => {
    clearInterval(id);
    process.stdout.write("\r\x1b[K");
  };
}
