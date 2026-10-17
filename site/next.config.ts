import type { NextConfig } from 'next'

const config: NextConfig = {
  output: 'export',
  trailingSlash: false,
  images: { unoptimized: true },
  agentRules: false,
}

export default config
