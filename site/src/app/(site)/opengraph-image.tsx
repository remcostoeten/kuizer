import { readFile } from 'node:fs/promises'
import { join } from 'node:path'

export const dynamic = 'force-static'
export const alt = 'kuizer: reclaim your disk from build output, caches and duplicates'
export const size = { width: 1200, height: 630 }
export const contentType = 'image/png'

export default async function OpenGraphImage() {
  const image = await readFile(join(process.cwd(), 'public/brand/sweep/social/opengraph-dark.png'))
  return new Response(new Uint8Array(image), { headers: { 'Content-Type': contentType } })
}
