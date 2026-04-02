import { describe, expect, it } from 'bun:test'
import { detect } from '../../src/detectors/unused-files.js'
import { makeConfig, makeCtx } from './fixture.js'

describe('unusedFiles', () => {
  it('flags files outside the reachable set', () => {
    const ctx = makeCtx(
      { '/src/dead.ts': '', '/src/live.ts': '' },
      { '/src/live.ts': '' },
    )
    const findings = detect(ctx, makeConfig())

    expect(findings).toHaveLength(1)
    expect(findings[0]?.file).toBe('/src/dead.ts')
    expect(findings[0]?.detectorId).toBe('unusedFiles')
  })

  it('produces no findings when all files are reachable', () => {
    const files = { '/src/a.ts': '', '/src/b.ts': '' }
    const ctx   = makeCtx(files, files)

    expect(detect(ctx, makeConfig())).toHaveLength(0)
  })

  it('respects ignore.files', () => {
    const ctx = makeCtx(
      { '/src/dead.ts': '', '/src/live.ts': '' },
      { '/src/live.ts': '' },
    )
    const config = makeConfig({ ignore: { files: ['/src/dead.ts'], symbols: [], paths: [] } })

    expect(detect(ctx, config)).toHaveLength(0)
  })

  it('uses configured severity', () => {
    const ctx = makeCtx(
      { '/src/dead.ts': '' },
      {},
    )
    const config = makeConfig({
      detectors: { unusedFiles: { severity: 'error' } },
    })

    expect(detect(ctx, config)[0]?.severity).toBe('error')
  })

  it('adds uncertainty context when analysis boundaries exist', () => {
    const ctx = {
      ...makeCtx(
        { '/src/dead.ts': '', '/src/live.ts': '' },
        { '/src/live.ts': '' },
      ),
      analysisBoundaries: [
        {
          file: '/src/live.ts',
          line: 2,
          col: 1,
          kind: 'dynamic-import' as const,
          message: 'Non-literal dynamic import cannot be resolved statically',
        },
      ],
    }

    const finding = detect(ctx, makeConfig())[0]
    expect(finding?.message).toContain('statically resolved')
    expect(finding?.context).toEqual({
      analysisBoundaryCount: 1,
      hasDynamicImportBoundary: true,
    })
  })
})
