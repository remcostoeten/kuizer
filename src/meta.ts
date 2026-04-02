import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

let cachedVersion: string | undefined;

function findPackageJson(startFileUrl: string): string {
  let dir = dirname(fileURLToPath(startFileUrl));

  while (true) {
    const candidate = join(dir, "package.json");
    if (existsSync(candidate)) {
      return candidate;
    }

    const parent = dirname(dir);
    if (parent === dir) {
      throw new Error("Unable to locate package.json");
    }

    dir = parent;
  }
}

export function getVersion(startFileUrl: string): string {
  if (cachedVersion) {
    return cachedVersion;
  }

  const packageJsonPath = findPackageJson(startFileUrl);
  const pkg = JSON.parse(readFileSync(packageJsonPath, "utf8")) as {
    version?: string;
  };

  if (!pkg.version) {
    throw new Error(`Missing version in ${packageJsonPath}`);
  }

  cachedVersion = pkg.version;
  return cachedVersion;
}
