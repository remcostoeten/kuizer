import { spawn } from 'node:child_process'
import { createServer } from 'node:net'

type Mode = 'dev' | 'start'

const DEFAULT_PORT = 3000
const MAX_PORT_ATTEMPTS = 50

function parseMode(value: string | undefined): Mode {
  if (value === 'dev' || value === 'start') return value
  throw new Error(`Unknown mode "${value}". Use "dev" or "start".`)
}

function isPortFree(port: number) {
  return new Promise<boolean>((resolve) => {
    const server = createServer()
    server.once('error', () => resolve(false))
    server.once('listening', () => server.close(() => resolve(true)))
    server.listen(port)
  })
}

async function findFreePort(preferred: number) {
  for (let port = preferred; port < preferred + MAX_PORT_ATTEMPTS; port++) {
    if (await isPortFree(port)) return port
  }
  throw new Error(`No free port found between ${preferred} and ${preferred + MAX_PORT_ATTEMPTS - 1}.`)
}

function openInBrowser(url: string) {
  const command =
    process.platform === 'darwin' ? 'open' : process.platform === 'win32' ? 'explorer' : 'xdg-open'
  const opener = spawn(command, [url], { stdio: 'ignore', detached: true })
  opener.once('spawn', () => console.log(`  ✓ opened ${url} in the browser`))
  opener.once('error', () => console.error(`  ✗ could not open the browser, visit ${url}`))
  opener.unref()
}

async function main() {
  const mode = parseMode(process.argv[2])
  const preferred = Number(process.env.PORT) || DEFAULT_PORT
  const port = await findFreePort(preferred)
  const url = `http://localhost:${port}`

  if (port !== preferred) {
    console.log(`Port ${preferred} is in use, using ${port} instead.`)
  }

  const child = spawn('bun', ['--bun', 'next', mode, '-p', String(port)], {
    stdio: ['ignore', 'inherit', 'inherit'],
    env: { ...process.env, PORT: String(port) },
  })

  function shutdown() {
    if (process.stdin.isTTY) process.stdin.setRawMode(false)
    child.kill('SIGINT')
  }

  child.on('exit', (code) => process.exit(code ?? 0))
  process.on('SIGINT', shutdown)
  process.on('SIGTERM', shutdown)

  if (!process.stdin.isTTY) return

  console.log(`\n  press o to open ${url} in the browser, q to quit\n`)
  process.stdin.setRawMode(true)
  process.stdin.setEncoding('utf8')
  process.stdin.on('data', (key: string) => {
    if (key === 'o') openInBrowser(url)
    // \u0003 is ctrl+c, which raw mode delivers as input instead of SIGINT
    if (key === 'q' || key === '\u0003') shutdown()
  })
}

main().catch((error: Error) => {
  console.error(error.message)
  process.exit(1)
})
