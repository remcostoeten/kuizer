import { Project } from "ts-morph";
import fg from "fast-glob";
import type { Config, Ctx } from "../types.js";

/** Initialises the ts-morph project, collects all source files, and runs BFS reachability from resolved entry points. */
export async function bootstrap(config: Config): Promise<Ctx> {
  const project = new Project({
    tsConfigFilePath: config.tsconfig,
    skipAddingFilesFromTsConfig: true,
  });

  const files = await fg(["**/*.ts", "**/*.tsx"], {
    cwd: process.cwd(),
    ignore: [
      "**/node_modules/**",
      `${config.outputDir}/**`,
      ...config.ignore.paths.map((p) => `${p}/**`),
      ...config.ignore.files,
    ],
    absolute: true,
  });

  files.forEach((f) => project.addSourceFileAtPath(f));

  const all = new Set(files);
  const entries = await resolveEntries(config, all);
  const reachable = bfs(project, entries);

  return { project, reachable, all };
}

async function resolveEntries(
  config: Config,
  all: ReadonlySet<string>,
): Promise<ReadonlySet<string>> {
  const patterns =
    config.entrypoints.length > 0
      ? [...config.entrypoints]
      : ["src/index.ts", "src/index.tsx", "index.ts"];

  const matched = await fg(patterns, { cwd: process.cwd(), absolute: true });
  return new Set(matched.filter((f) => all.has(f)));
}

function bfs(
  project: Project,
  entries: ReadonlySet<string>,
): ReadonlySet<string> {
  const visited = new Set<string>();
  const queue = [...entries];

  while (queue.length > 0) {
    const path = queue.shift()!;
    if (visited.has(path)) continue;
    visited.add(path);

    const sf = project.getSourceFile(path);
    if (!sf) continue;

    for (const decl of sf.getImportDeclarations()) {
      const resolved = decl.getModuleSpecifierSourceFile();
      if (!resolved) continue;
      const next = resolved.getFilePath();
      if (!visited.has(next)) queue.push(next);
    }
  }

  return visited;
}
