import type { Project } from "ts-morph";

export type Severity = "error" | "warn" | "info";

export type Framework = "nextjs-app" | "nextjs-pages" | "vite" | "none";

/** A single detector result, immutable once produced. */
export type Finding = Readonly<{
  detectorId: string;
  severity: Severity;
  file: string;
  line: number;
  col: number;
  message: string;
  symbol?: string;
  fixable: boolean;
}>;

export type DetectorOpts = Readonly<{
  enabled?: boolean;
  severity?: Severity;
  ignore?: readonly string[];
}>;

export type Ignore = Readonly<{
  files: readonly string[];
  symbols: readonly string[];
  paths: readonly string[];
}>;

export type Detectors = Readonly<{
  unusedFiles?: DetectorOpts;
  unusedExports?: DetectorOpts;
}>;

/** Fully resolved config — all fields present, no optionals at top level. */
export type Config = Readonly<{
  tsconfig: string;
  entrypoints: readonly string[];
  framework: Framework;
  countTestsAsConsumers: boolean;
  allowedExports: readonly string[];
  ignore: Ignore;
  detectors: Detectors;
  outputDir: string;
}>;

/** Shared analysis context produced by the engine, consumed by detectors. */
export type Ctx = Readonly<{
  project: Project;
  reachable: ReadonlySet<string>;
  all: ReadonlySet<string>;
}>;
