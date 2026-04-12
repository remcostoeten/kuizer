import { describe, expect, it } from 'bun:test'
import {
	collectFlagValidationErrors,
	normalizeArgv,
	resolveReporter,
} from '../../src/cli/index.js'

describe('kuizer CLI dispatch helpers', () => {
	it('rewrites command aliases and flag-driven command entries', () => {
		expect(normalizeArgv(['-c', 'configs/dev.json'])).toEqual([
			'detect',
			'--config',
			'configs/dev.json',
		])
		expect(normalizeArgv(['--fix'])).toEqual(['fix'])
		expect(normalizeArgv(['json'])).toEqual(['detect', '--json'])
		expect(normalizeArgv(['-v'])).toEqual(['version'])
	})

	it('reports empty flag values for dispatch validation', () => {
		const errors = collectFlagValidationErrors('detect', {
			config: '',
			tsconfig: '',
			only: '',
			skip: '',
			reporter: '',
			json: undefined,
			filter: '',
			excludePattern: undefined,
			interactive: undefined,
			includeUi: undefined,
		})

		expect(errors).toHaveLength(6)
		expect(errors).toContain('Config path cannot be empty. Use: kuizer -c <path>')
		expect(errors).toContain('Filter cannot be empty. Use: kuizer -f <types>')
	})

	it('selects the json reporter when the json flag is set', () => {
		expect(resolveReporter({ json: true, reporter: undefined } as never)).toBe('json')
		expect(resolveReporter({ json: false, reporter: 'terminal' } as never)).toBe('terminal')
	})

	it('rejects conflicting write and dry-run flags', () => {
		const errors = collectFlagValidationErrors('fix', {
			config: undefined,
			tsconfig: undefined,
			only: undefined,
			skip: undefined,
			dryRun: true,
			write: true,
			noBackup: undefined,
			includeUi: undefined,
		})

		expect(errors).toContain('Cannot combine --write with --dryRun/--dry. Choose one mode.')
	})

	it('normalizes the include-ui flag for detect and fix commands', () => {
		expect(normalizeArgv(['detect', '--include-ui'])).toEqual(['detect', '--includeUi'])
		expect(normalizeArgv(['fix', '--include-ui'])).toEqual(['fix', '--includeUi'])
	})
})
