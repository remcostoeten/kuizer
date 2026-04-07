import { describe, expect, it } from 'bun:test'
import { detect } from '../../src/detectors/unused-imports.js'
import { registry } from '../../src/detectors/registry.js'
import { makeConfig, makeCtx } from './fixture.js'

describe('unusedImports', () => {
	it('flags unused named, default, namespace, and aliased imports', () => {
		const ctx = makeCtx(
			{
				'/src/live.ts': `
					import Foo, { used, dead as renamedDead } from '/src/deps'
					import * as Utils from '/src/deps'

					used()
					console.log(Utils)
					console.log(Foo)
				`,
				'/src/deps.ts': `
					export default function Foo() {}
					export function used() {}
					export function dead() {}
				`,
			},
			{
				'/src/live.ts': `
					import Foo, { used, dead as renamedDead } from '/src/deps'
					import * as Utils from '/src/deps'

					used()
					console.log(Utils)
					console.log(Foo)
				`,
				'/src/deps.ts': `
					export default function Foo() {}
					export function used() {}
					export function dead() {}
				`,
			},
		)

		const findings = detect(ctx, makeConfig())
		expect(findings).toHaveLength(1)
		expect(findings[0]?.detectorId).toBe('unusedImports')
		expect(findings[0]?.symbol).toBe('renamedDead')
		expect(findings[0]?.fixable).toBe(false)
		expect(findings[0]?.context).toEqual({
			consumerCount: 0,
		})
	})

	it('does not flag imports that are used in value or type positions', () => {
		const ctx = makeCtx(
			{
				'/src/live.ts': `
					import type { Foo } from '/src/deps'
					import { Bar } from '/src/deps'

					const value: Foo = { ok: true }
					console.log(Bar, value)
				`,
				'/src/deps.ts': `
					export type Foo = { ok: boolean }
					export const Bar = 1
				`,
			},
			{
				'/src/live.ts': `
					import type { Foo } from '/src/deps'
					import { Bar } from '/src/deps'

					const value: Foo = { ok: true }
					console.log(Bar, value)
				`,
				'/src/deps.ts': `
					export type Foo = { ok: boolean }
					export const Bar = 1
				`,
			},
		)

		expect(detect(ctx, makeConfig())).toHaveLength(0)
	})

	it('is registered in the detector registry', () => {
		expect(registry.unusedImports).toBe(detect)
	})
})
