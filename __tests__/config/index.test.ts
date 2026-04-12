import { describe, expect, it } from 'bun:test'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { loadConfig } from '../../src/config/index.js'

describe('loadConfig', () => {
	it('returns fully resolved config with defaults when no file exists', async () => {
		const config = await loadConfig('/tmp')

		expect(config.framework).toBe('none')
		expect(config.tsconfig).toBe('/tmp/tsconfig.json')
		expect(config.outputDir).toBe('/tmp/.kuizer')
		expect(config.entrypoints).toBeArray()
	})

	it('uses the nearest tsconfig.json upward when config does not set one', async () => {
		const cwd = mkdtempSync(join(tmpdir(), 'kuizer-config-nearest-'))
		const nested = join(cwd, 'packages', 'app')

		try {
			mkdirSync(nested, { recursive: true })
			writeFileSync(
				join(cwd, 'tsconfig.json'),
				JSON.stringify({
					compilerOptions: { module: 'NodeNext' },
					include: ['src'],
				})
			)

			const config = await loadConfig(nested)

			expect(config.tsconfig).toBe(join(cwd, 'tsconfig.json'))
		} finally {
			rmSync(cwd, { recursive: true, force: true })
		}
	})

	it('loads a local .kuizerrc file during search', async () => {
		const cwd = mkdtempSync(join(tmpdir(), 'kuizer-config-rc-'))

		try {
			writeFileSync(
				join(cwd, '.kuizerrc'),
				JSON.stringify({
					framework: 'nextjs-pages',
					dryRunByDefault: false,
				})
			)

			const config = await loadConfig(cwd)

			expect(config.framework).toBe('nextjs-pages')
			expect(config.dryRunByDefault).toBe(false)
		} finally {
			rmSync(cwd, { recursive: true, force: true })
		}
	})

	it('applies nextjs preset ui ignore paths by default', async () => {
		const cwd = mkdtempSync(join(tmpdir(), 'kuizer-config-next-preset-'))

		try {
			mkdirSync(join(cwd, 'src', 'app'), { recursive: true })
			writeFileSync(join(cwd, 'src', 'app', 'page.tsx'), 'export default function Page() { return null }\n')
			writeFileSync(
				join(cwd, 'tsconfig.json'),
				JSON.stringify({
					compilerOptions: { jsx: 'react-jsx' },
					include: ['src/**/*'],
				})
			)

			const config = await loadConfig(cwd)

			expect(config.framework).toBe('nextjs-app')
			expect(config.ignore.paths).toContain('**/ui/**')
			expect(config.ignore.paths).toContain('**/components/ui/**')
		} finally {
			rmSync(cwd, { recursive: true, force: true })
		}
	})

	it('honors an explicit tsconfig override over config and search', async () => {
		const cwd = mkdtempSync(join(tmpdir(), 'kuizer-config-override-'))
		const configPath = join(cwd, 'kuizer.config.json')

		try {
			writeFileSync(
				configPath,
				JSON.stringify({
					framework: 'nextjs-pages',
					tsconfig: './config-tsconfig.json',
				})
			)
			writeFileSync(join(cwd, 'override-tsconfig.json'), '{}')

			const config = await loadConfig(cwd, './kuizer.config.json', './override-tsconfig.json')

			expect(config.framework).toBe('nextjs-pages')
			expect(config.tsconfig).toBe(join(cwd, 'override-tsconfig.json'))
		} finally {
			rmSync(cwd, { recursive: true, force: true })
		}
	})

	it('rebases relative paths from an explicit config file', async () => {
		const cwd = mkdtempSync(join(tmpdir(), 'kuizer-config-base-'))
		const configDir = join(cwd, 'config')
		const configPath = join(configDir, 'kuizer.config.js')

		try {
			mkdirSync(configDir)

			writeFileSync(
				configPath,
				[
					"module.exports = {",
					"  tsconfig: '../tsconfig.json',",
					"  entrypoints: ['../src/index.ts'],",
					"  outputDir: '../.kuizer-out',",
					'};',
				].join('\n')
			)

			const config = await loadConfig(cwd, './config/kuizer.config.js')

			expect(config.tsconfig).toBe(join(cwd, 'tsconfig.json'))
			expect(config.entrypoints).toEqual([join(cwd, 'src/index.ts')])
			expect(config.outputDir).toBe(join(cwd, '.kuizer-out'))
		} finally {
			rmSync(cwd, { recursive: true, force: true })
		}
	})

	it('rejects invalid config values', async () => {
		const cwd = mkdtempSync(join(tmpdir(), 'kuizer-config-invalid-'))
		const configPath = join(cwd, 'kuizer.config.json')

		try {
			writeFileSync(
				configPath,
				JSON.stringify({
					framework: 'angular',
				})
			)

			await expect(loadConfig(cwd)).rejects.toThrow('invalid framework')
		} finally {
			rmSync(cwd, { recursive: true, force: true })
		}
	})

	it('rejects invalid detector option types', async () => {
		const cwd = mkdtempSync(join(tmpdir(), 'kuizer-config-detectors-'))
		const configPath = join(cwd, 'kuizer.config.json')

		try {
			writeFileSync(
				configPath,
				JSON.stringify({
					detectors: {
						unusedImports: {
							enabled: 'yes',
							severity: 'loud',
						},
					},
				})
			)

			await expect(loadConfig(cwd)).rejects.toThrow('detectors.unusedImports.enabled')
		} finally {
			rmSync(cwd, { recursive: true, force: true })
		}
	})
})
