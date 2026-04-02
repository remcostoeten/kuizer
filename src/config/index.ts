import { cosmiconfig } from "cosmiconfig";
import { dirname, isAbsolute, resolve } from "node:path";
import type { Config, Framework } from "../types.js";
import { defaults } from "./defaults.js";
import { preset } from "./presets.js";

type Raw = Partial<{
  tsconfig: string;
  entrypoints: string[];
  framework: Framework;
  countTestsAsConsumers: boolean;
  allowedExports: string[];
  outputDir: string;
  ignore: Partial<{
    files: string[];
    symbols: string[];
    paths: string[];
  }>;
  detectors: Config["detectors"];
}>;

function resolveFrom(baseDir: string, value: string): string {
  return isAbsolute(value) ? value : resolve(baseDir, value);
}

/** Merges user-supplied raw config with framework preset and defaults. */
function mergeConfig(raw: Raw, baseDir: string): Config {
  const framework = raw.framework ?? defaults.framework;
  const p = preset(framework);

  return {
    tsconfig: resolveFrom(baseDir, raw.tsconfig ?? defaults.tsconfig),
    framework,
    countTestsAsConsumers:
      raw.countTestsAsConsumers ?? defaults.countTestsAsConsumers,
    outputDir: resolveFrom(baseDir, raw.outputDir ?? defaults.outputDir),
    entrypoints: [...p.entrypoints, ...(raw.entrypoints ?? [])].map((entry) =>
      resolveFrom(baseDir, entry),
    ),
    allowedExports: [...p.allowedExports, ...(raw.allowedExports ?? [])],
    ignore: {
      files: [...(raw.ignore?.files ?? [])],
      symbols: [...(raw.ignore?.symbols ?? [])],
      paths: [...(raw.ignore?.paths ?? [])],
    },
    detectors: raw.detectors ?? {},
  };
}

/** Discovers and loads kuizer config via cosmiconfig, returning a fully resolved Config. */
export async function loadConfig(
  cwd = process.cwd(),
  configPath?: string,
): Promise<Config> {
  const explorer = cosmiconfig("kuizer", {
    searchPlaces: [
      "kuizer.config.ts",
      "kuizer.config.js",
      ".kuizerrc.ts",
      ".kuizerrc.js",
      ".kuizerrc.json",
      "package.json",
    ],
  });

  const result = configPath
    ? await explorer.load(resolve(cwd, configPath))
    : await explorer.search(cwd);

  const loadedPath = result?.filepath;
  const baseDir = loadedPath ? dirname(loadedPath) : cwd;

  return mergeConfig((result?.config as Raw | undefined) ?? {}, baseDir);
}
