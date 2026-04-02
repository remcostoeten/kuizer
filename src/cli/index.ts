#!/usr/bin/env node

import { cli, command } from "cleye";
import { loadConfig } from "../config/index.js";
import { registry as detectorRegistry } from "../detectors/registry.js";
import { bootstrap } from "../engine/index.js";
import { getVersion } from "../meta.js";
import { registry as reporterRegistry } from "../reporters/registry.js";
import type { Config, Finding } from "../types.js";

const version = getVersion(import.meta.url);

type DetectorId = keyof typeof detectorRegistry;
type ReporterName = keyof typeof reporterRegistry;

type DetectFlags = {
  config: string | undefined;
  tsconfig: string | undefined;
  reporter: string | undefined;
  only: string | undefined;
  skip: string | undefined;
  json: boolean | undefined;
};

function parseCsv(input?: string): string[] {
  if (!input) return [];

  return input
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);
}

function resolveReporter(flags: DetectFlags): ReporterName {
  if (flags.json) return "json";

  const reporter = flags.reporter ?? "terminal";
  if (reporter in reporterRegistry) {
    return reporter as ReporterName;
  }

  throw new Error(`Unknown reporter '${reporter}'`);
}

function resolveDetectorIds(config: Config, flags: DetectFlags): DetectorId[] {
  const available = Object.keys(detectorRegistry) as DetectorId[];
  const only = parseCsv(flags.only);
  const skip = new Set(parseCsv(flags.skip));
  const enabled = available.filter((id) => config.detectors[id]?.enabled !== false);

  for (const detectorId of [...only, ...skip]) {
    if (!available.includes(detectorId as DetectorId)) {
      throw new Error(`Unknown detector '${detectorId}'`);
    }
  }

  return (only.length > 0 ? only : enabled).filter(
    (id): id is DetectorId => !skip.has(id),
  );
}

function exitCode(findings: readonly Finding[]): number {
  return findings.some((finding) => finding.severity === "error") ? 1 : 0;
}

export async function runDetect(flags: DetectFlags): Promise<number> {
  const startedAt = Date.now();
  const config = {
    ...(await loadConfig(process.cwd(), flags.config)),
    ...(flags.tsconfig ? { tsconfig: flags.tsconfig } : {}),
  } satisfies Config;

  const ctx = await bootstrap(config);
  const detectors = resolveDetectorIds(config, flags);
  const findings = detectors.flatMap((id) => detectorRegistry[id](ctx, config));
  const reporter = reporterRegistry[resolveReporter(flags)];

  reporter(findings, Date.now() - startedAt);
  return exitCode(findings);
}

export async function main(argv = process.argv.slice(2)): Promise<number> {
  const detect = command({
    name: "detect",
    flags: {
      config: {
        type: String,
        description: "Path to config file",
      },
      tsconfig: {
        type: String,
        description: "Override tsconfig path",
      },
      reporter: {
        type: String,
        description: "Reporter to use (terminal or json)",
      },
      only: {
        type: String,
        description: "Comma-separated list of detectors to run",
      },
      skip: {
        type: String,
        description: "Comma-separated list of detectors to skip",
      },
      json: {
        type: Boolean,
        description: "Shorthand for --reporter json",
      },
    },
  });

  const parsed = cli(
    {
      name: "kuizer",
      version,
      help: {
        description: "TypeScript dead code intelligence for large codebases.",
      },
      commands: [detect],
    },
    undefined,
    argv,
  );

  if (!("command" in parsed)) {
    (parsed as { showHelp(): void }).showHelp();
    return 1;
  }

  if (parsed.command === "detect") {
    return runDetect(parsed.flags);
  }

  (parsed as { showHelp(): void }).showHelp();
  return 1;
}

try {
  process.exitCode = await main();
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`kuizer: ${message}`);
  process.exitCode = 2;
}
