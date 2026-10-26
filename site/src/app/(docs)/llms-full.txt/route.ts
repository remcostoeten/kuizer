import { docsLlms } from '@/modules/docs/server/llms'

export const dynamic = 'force-static'

export async function GET() {
  return new Response(await docsLlms.full(), {
    headers: { 'Content-Type': 'text/markdown; charset=utf-8' },
  })
}
