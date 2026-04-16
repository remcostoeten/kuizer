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
  it("includes all workspace files, even if outside tsconfig include (for monorepo sibling support)", async () => {
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

      expect(files).toEqual(["src/index.ts", "src/lib.test.ts", "src/lib.ts", "test/ignored.ts", "tooling.ts"]);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("follows reachability from sibling apps in a monorepo", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        include: ["src/**/*.ts"],
      }),
      "src/shared.ts": "export const shared = true\n",
      "apps/mobile/src/main.ts": "import { shared } from '../../../src/shared.js'\nconsole.log(shared)\n",
    });

    try {
      const ctx = await bootstrap(makeConfig(cwd));
      const reachable = [...ctx.reachable]
        .map((file) => file.replace(cwd + "/", ""))
        .sort();

      expect(reachable).toContain("src/shared.ts");
      expect(reachable).toContain("apps/mobile/src/main.ts");
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("distinguishes between prod and test reachability", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        include: ["src/**/*.ts"],
      }),
      "src/prod.ts": "export const prod = true\n",
      "src/only-test.ts": "export const testOnly = true\n",
      "src/main.ts": "import { prod } from './prod.js'\n",
      "__tests__/index.test.ts": "import { testOnly } from '../src/only-test.js'\n",
    });

    try {
      const ctx = await bootstrap(makeConfig(cwd, { entrypoints: [join(cwd, "src/main.ts")] }));
      
      const reachable = [...ctx.reachable]
        .map((file) => file.replace(cwd + "/", ""))
        .sort();
      const testReachable = [...ctx.testReachable]
        .map((file) => file.replace(cwd + "/", ""))
        .sort();

      expect(reachable).toContain("src/prod.ts");
      expect(reachable).not.toContain("src/only-test.ts");
      
      expect(testReachable).toContain("src/only-test.ts");
      expect(testReachable).toContain("__tests__/index.test.ts");
      expect(testReachable).not.toContain("src/prod.ts"); // Should be in prod reachable instead
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("infers entrypoints from files with no internal importers (excluding tests)", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        include: ["src/**/*.ts"],
      }),
      "src/cli.ts": "import './shared.js'\n",
      "src/shared.ts": "export const shared = true\n",
      "src/orphan.ts": "export const orphan = true\n",
      "__tests__/some.test.ts": "import './shared.js'\n",
    });

    try {
      const ctx = await bootstrap(makeConfig(cwd));
      const reachable = [...ctx.reachable]
        .map((file) => file.replace(cwd + "/", ""))
        .sort();

      // __tests__/some.test.ts should NOT be a prod entrypoint, but src/cli.ts and src/orphan.ts should be.
      expect(reachable).toEqual(["src/cli.ts", "src/orphan.ts", "src/shared.ts"]);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("ignores nested .next and node_modules directories", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        include: ["src/**/*.ts"],
      }),
      "src/index.ts": "export const a = 1\n",
      "apps/web/.next/static/chunks/main.js": "console.log('generated')\n",
      "apps/web/node_modules/dep/index.ts": "export const dep = 1\n",
    });

    try {
      const ctx = await bootstrap(makeConfig(cwd));
      const files = [...ctx.all].map((file) => file.replace(cwd + "/", "")).sort();

      expect(files).toEqual(["src/index.ts"]);
      expect(files).not.toContain("apps/web/.next/static/chunks/main.js");
      expect(files).not.toContain("apps/web/node_modules/dep/index.ts");
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

  it("adds out-of-program tests when countTestsAsConsumers is enabled", async () => {
    const cwd = createWorkspace({
      "tsconfig.json": JSON.stringify({
        compilerOptions: {
          target: "ES2020",
          module: "ESNext",
          moduleResolution: "Bundler",
        },
        include: ["src/**/*.ts"],
      }),
      "src/index.ts": "export function main() { return 1 }\n",
      "__tests__/index.test.ts": "import { main } from '../src/index.js'\nmain()\n",
    });

    try {
      const ctx = await bootstrap(
        makeConfig(cwd, {
          countTestsAsConsumers: true,
        }),
      );

      const files = [...ctx.all].map((file) => file.replace(cwd + "/", "")).sort();
      const reachable = [...ctx.reachable]
        .map((file) => file.replace(cwd + "/", ""))
        .sort();

      expect(files).toEqual(["__tests__/index.test.ts", "src/index.ts"]);
      expect(reachable).toEqual(["__tests__/index.test.ts", "src/index.ts"]);
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });

  it("emits bootstrap progress events", async () => {
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
    });

    const phases: string[] = [];

    try {
      await bootstrap(makeConfig(cwd), {
        onProgress(event) {
          phases.push(event.phase);
        },
      });

      expect(phases).toContain("load-project");
      expect(phases).toContain("collect-files");
      expect(phases).toContain("resolve-entries");
      expect(phases).toContain("analyze-file");
      expect(phases.at(-1)).toBe("complete");
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });
});
