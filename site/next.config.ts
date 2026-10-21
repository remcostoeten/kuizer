import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import type { NextConfig } from 'next'

function readWorkspaceVersion() {
  const manifest = readFileSync(join(process.cwd(), '..', 'Cargo.toml'), 'utf8')
  // the version under [workspace.package], which every crate inherits
  const match = /\[workspace\.package\][^[]*?\bversion\s*=\s*"([^"]+)"/.exec(manifest)
  if (!match?.[1]) throw new Error('No [workspace.package] version found in ../Cargo.toml')
  return match[1]
}

const config: NextConfig = {
  env: { NEXT_PUBLIC_KUIZER_VERSION: readWorkspaceVersion() },
  output: 'export',
  trailingSlash: false,
  images: { unoptimized: true },
  agentRules: false,
}

export default config
