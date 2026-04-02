import { describe, it, expect, beforeAll } from 'bun:test'
import { resolve } from 'path'
import { bootstrap } from '../src/engine/index.js'
import { detect as unusedFiles } from '../src/detectors/unused-files.js'
import { detect as unusedExports } from '../src/detectors/unused-exports.js'
import type { Config, Ctx, Finding } from '../src/types.js'
import { defaults } from '../src/config/defaults.js'

const FIXTURE = resolve(import.meta.dir, '../tests/fixtures/sample')

const config: Config = {
  ...defaults,
  tsconfig:    `${FIXTURE}/tsconfig.json`,
  entrypoints: [`${FIXTURE}/src/index.ts`],
  framework:   'none',
}

function filenames(findings: readonly Finding[], detector: string): string[] {
  return findings
    .filter(f => f.detectorId === detector)
    .map(f => f.file.replace(FIXTURE + '/', ''))
    .sort()
}

function symbols(findings: readonly Finding[], detector: string): string[] {
  return findings
    .filter(f => f.detectorId === detector)
    .map(f => f.symbol ?? f.file)
    .sort()
}

describe('e2e — sample fixture', () => {
  let ctx: Ctx

  beforeAll(async () => {
    ctx = await bootstrap(config)
  })

  it('bootstraps and resolves a non-empty project', () => {
    expect(ctx.all.size).toBeGreaterThan(0)
    expect(ctx.reachable.size).toBeGreaterThan(0)
    expect(ctx.reachable.size).toBeLessThan(ctx.all.size)
  })

  it('reachable set contains exactly the live files', () => {
    const names = [...ctx.reachable]
      .map(f => f.replace(FIXTURE + '/', ''))
      .sort()

    expect(names).toEqual([
      'src/index.ts',
      'src/lib/greet.ts',
      'src/lib/logger.ts',
      'src/types/index.ts',
    ])
  })

  it('unusedFiles flags exactly the unreachable files', () => {
    const names = filenames(unusedFiles(ctx, config), 'unusedFiles')

    expect(names).toEqual([
      'src/components/Button.ts',
      'src/components/Modal.ts',
      'src/utils/format.ts',
      'src/utils/math.ts',
    ])
  })

  it('unusedExports flags dead exports in reachable files', () => {
    const found = symbols(unusedExports(ctx, config), 'unusedExports')

    expect(found).toEqual([
      'FileLogger',
      'PaginatedResponse',
      'Timestamp',
      'shout',
    ])
  })

  it('unusedExports does not flag consumed exports', () => {
    const found = symbols(unusedExports(ctx, config), 'unusedExports')

    expect(found).not.toContain('greet')
    expect(found).not.toContain('Logger')
    expect(found).not.toContain('ID')
    expect(found).not.toContain('Result')
  })

  it('all findings reference files within the fixture', () => {
    const findings = [
      ...unusedFiles(ctx, config),
      ...unusedExports(ctx, config),
    ]

    for (const f of findings) {
      expect(f.file).toStartWith(FIXTURE)
    }
  })
})
