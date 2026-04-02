import { describe, expect, it } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { defaults } from "../../src/config/defaults.js";
import { bootstrap } from "../../src/engine/index.js";
import type { Config } from "../../src/types.js";

function createWorkspace(files: Record<string, string>): string {
  const cwd = mkdtempSync(join(tmpdir(), "kuizer-engine-"));

  for (const [relativePath, contents] of Object.entries(files)) {
    const target = join(cwd, relativePath);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, contents);
  }

  return cwd;
}

function makeConfig(cwd: string, overrides: Partial<Config> = {}): Config {
  return {
    ...defaults,
    tsconfig: join(cwd, "tsconfig.json"),
    ...overrides,
  };
}

describe("bootstrap", () => {
  it("limits source files to the tsconfig program", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        compilerOptions: {
          target: "ES2020",
          module: "ESNext",
          moduleResolution: "Bundler",
        },
        include: ["src/**/*.ts"],
      }),
      "src/index.ts": "import './lib.js'\n",
      "src/lib.ts": "export const value = 1\n",
      "src/lib.test.ts": "export const ignored = true\n",
      "test/ignored.ts": "export const ignored = true\n",
      "tooling.ts": "export const tooling = true\n",
    });

    try {
      const ctx = await bootstrap(makeConfig(cwd));
      const files = [...ctx.all].map((file) => file.replace(cwd + "/", "")).sort();

      expect(files).toEqual(["src/index.ts", "src/lib.ts"]);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("infers entrypoints from files with no internal importers", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        compilerOptions: {
          target: "ES2020",
          module: "ESNext",
          moduleResolution: "Bundler",
        },
        include: ["src/**/*.ts"],
      }),
      "src/cli.ts": "import './shared.js'\n",
      "src/shared.ts": "export const shared = true\n",
      "src/orphan.ts": "export const orphan = true\n",
    });

    try {
      const ctx = await bootstrap(makeConfig(cwd));
      const reachable = [...ctx.reachable]
        .map((file) => file.replace(cwd + "/", ""))
        .sort();

      expect(reachable).toEqual(["src/cli.ts", "src/orphan.ts", "src/shared.ts"]);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("expands glob entrypoints before reachability analysis", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        compilerOptions: {
          target: "ES2020",
          module: "ESNext",
          moduleResolution: "Bundler",
        },
        include: ["src/**/*.ts"],
      }),
      "src/index.ts": "import './lib.js'\n",
      "src/lib.ts": "export const value = 1\n",
      "src/dead.ts": "export const dead = true\n",
    });

    try {
      const ctx = await bootstrap(
        makeConfig(cwd, {
          entrypoints: [join(cwd, "src", "index.*")],
        }),
      );

      const reachable = [...ctx.reachable]
        .map((file) => file.replace(cwd + "/", ""))
        .sort();

      expect(reachable).toEqual(["src/index.ts", "src/lib.ts"]);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("follows re-export edges during reachability analysis", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        compilerOptions: {
          target: "ES2020",
          module: "ESNext",
          moduleResolution: "Bundler",
        },
        include: ["src/**/*.ts"],
      }),
      "src/index.ts": "import { value } from './barrel.js'\nconsole.log(value)\n",
      "src/barrel.ts": "export { value } from './origin.js'\n",
      "src/origin.ts": "export const value = 1\n",
    });

    try {
      const ctx = await bootstrap(makeConfig(cwd));
      const reachable = [...ctx.reachable]
        .map((file) => file.replace(cwd + "/", ""))
        .sort();

      expect(reachable).toEqual(["src/barrel.ts", "src/index.ts", "src/origin.ts"]);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("follows export-star edges during reachability analysis", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        compilerOptions: {
          target: "ES2020",
          module: "ESNext",
          moduleResolution: "Bundler",
        },
        include: ["src/**/*.ts"],
      }),
      "src/index.ts": "import { value } from './barrel.js'\nconsole.log(value)\n",
      "src/barrel.ts": "export * from './origin.js'\n",
      "src/origin.ts": "export const value = 1\n",
    });

    try {
      const ctx = await bootstrap(makeConfig(cwd));
      const reachable = [...ctx.reachable]
        .map((file) => file.replace(cwd + "/", ""))
        .sort();

      expect(reachable).toEqual(["src/barrel.ts", "src/index.ts", "src/origin.ts"]);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("follows string-literal dynamic imports during reachability analysis", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        compilerOptions: {
          target: "ES2020",
          module: "ESNext",
          moduleResolution: "Bundler",
        },
        include: ["src/**/*.ts"],
      }),
      "src/index.ts": "await import('./lazy.js')\n",
      "src/lazy.ts": "export const lazy = true\n",
      "src/dead.ts": "export const dead = true\n",
    });

    try {
      const ctx = await bootstrap(makeConfig(cwd));
      const reachable = [...ctx.reachable]
        .map((file) => file.replace(cwd + "/", ""))
        .sort();

      expect(reachable).toEqual(["src/index.ts", "src/lazy.ts"]);
      expect(ctx.analysisBoundaries).toHaveLength(0);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("records non-literal dynamic imports as analysis boundaries", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        compilerOptions: {
          target: "ES2020",
          module: "ESNext",
          moduleResolution: "Bundler",
        },
        include: ["src/**/*.ts"],
      }),
      "src/index.ts": [
        "const name = 'lazy'",
        "await import(`./${name}.js`)",
      ].join("\n"),
      "src/lazy.ts": "export const lazy = true\n",
    });

    try {
      const ctx = await bootstrap(makeConfig(cwd));

      expect([...ctx.reachable].map((file) => file.replace(cwd + "/", "")).sort()).toEqual(["src/index.ts"]);
      expect(ctx.analysisBoundaries).toHaveLength(1);
      expect(ctx.analysisBoundaries[0]?.kind).toBe("dynamic-import");
      expect(ctx.analysisBoundaries[0]?.file).toBe(join(cwd, "src/index.ts"));
      expect(ctx.analysisBoundaries[0]?.line).toBe(2);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });
});
