import { cosmiconfig } from "cosmiconfig";
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

/** Merges user-supplied raw config with framework preset and defaults. */
function mergeConfig(raw: Raw): Config {
  const framework = raw.framework ?? defaults.framework;
  const p = preset(framework);

  return {
    tsconfig: raw.tsconfig ?? defaults.tsconfig,
    framework,
    countTestsAsConsumers:
      raw.countTestsAsConsumers ?? defaults.countTestsAsConsumers,
    outputDir: raw.outputDir ?? defaults.outputDir,
    entrypoints: [...p.entrypoints, ...(raw.entrypoints ?? [])],
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
export async function loadConfig(cwd = process.cwd()): Promise<Config> {
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

  const result = await explorer.search(cwd);
  return mergeConfig((result?.config as Raw | undefined) ?? {});
}
