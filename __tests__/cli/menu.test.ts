import { EventEmitter } from 'node:events'
import { describe, expect, it } from 'bun:test'
import { runMainMenu } from '../../src/cli/menu.js'
import type { Config } from '../../src/types.js'

class TestInput extends EventEmitter {
	isTTY = true
	rawMode = false
	resumed = false
	paused = false

	setRawMode(value: boolean): void {
		this.rawMode = value
	}

	resume(): this {
		this.resumed = true
		this.paused = false
		return this
	}

	pause(): this {
		this.paused = true
		this.resumed = false
		return this
	}
}

class TestOutput extends EventEmitter {
	isTTY = true
	columns = 120
	rows = 24
	buffer = ''

	write(chunk: string): boolean {
		this.buffer += chunk
		return true
	}
}

const TEST_CONFIG: Config = {
	tsconfig: '/repo/tsconfig.json',
	entrypoints: [],
	framework: 'nextjs-app',
	countTestsAsConsumers: false,
	allowedExports: [],
	ignore: {
		files: ['next.config.ts'],
		symbols: [],
		paths: ['**/components/ui/**'],
	},
	detectors: {},
	outputDir: '.kuizer',
	dryRunByDefault: true,
}

function press(
	input: TestInput,
	sequence: string,
	key: { name?: string; ctrl?: boolean } = {}
): void {
	input.emit('keypress', sequence, { ctrl: false, ...key })
}

describe('main menu', () => {
	it('renders detect editor with complex flag fields', async () => {
		const input = new TestInput()
		const output = new TestOutput()

		const session = runMainMenu({
			input: input as unknown as NodeJS.ReadStream,
			output: output as unknown as NodeJS.WriteStream,
			config: TEST_CONFIG,
			version: '0.1.1',
		})

		setTimeout(() => press(input, '\r', { name: 'return' }), 0)
		setTimeout(() => press(input, 'q', { name: 'q' }), 10)

		await expect(session).resolves.toBe(0)
		expect(output.buffer).toContain('Current Command: kuizer detect')
		expect(output.buffer).toContain('Only')
		expect(output.buffer).toContain('Exclude Pattern')
		expect(output.buffer).toContain('Interactive TUI')
	})

	it('updates the detect command preview when toggling reporter and json', async () => {
		const input = new TestInput()
		const output = new TestOutput()

		const session = runMainMenu({
			input: input as unknown as NodeJS.ReadStream,
			output: output as unknown as NodeJS.WriteStream,
			config: TEST_CONFIG,
			version: '0.1.1',
		})

		setTimeout(() => press(input, '\r', { name: 'return' }), 0)
		setTimeout(() => press(input, '', { name: 'down' }), 10)
		setTimeout(() => press(input, '', { name: 'down' }), 20)
		setTimeout(() => press(input, '', { name: 'down' }), 30)
		setTimeout(() => press(input, '', { name: 'down' }), 40)
		setTimeout(() => press(input, '', { name: 'down' }), 50)
		setTimeout(() => press(input, '', { name: 'down' }), 60)
		setTimeout(() => press(input, '\r', { name: 'return' }), 70)
		setTimeout(() => press(input, '', { name: 'down' }), 80)
		setTimeout(() => press(input, '\r', { name: 'return' }), 90)
		setTimeout(() => press(input, 'q', { name: 'q' }), 100)

		await expect(session).resolves.toBe(0)
		expect(output.buffer).toContain('kuizer detect --reporter json')
		expect(output.buffer).toContain('kuizer detect --json')
	})

	it('updates the fix command preview when toggling write mode and no-backup', async () => {
		const input = new TestInput()
		const output = new TestOutput()

		const session = runMainMenu({
			input: input as unknown as NodeJS.ReadStream,
			output: output as unknown as NodeJS.WriteStream,
			config: TEST_CONFIG,
			version: '0.1.1',
		})

		setTimeout(() => press(input, '', { name: 'down' }), 0)
		setTimeout(() => press(input, '\r', { name: 'return' }), 10)
		setTimeout(() => press(input, '', { name: 'down' }), 20)
		setTimeout(() => press(input, '', { name: 'down' }), 30)
		setTimeout(() => press(input, '', { name: 'down' }), 40)
		setTimeout(() => press(input, '', { name: 'down' }), 50)
		setTimeout(() => press(input, '\r', { name: 'return' }), 60)
		setTimeout(() => press(input, '', { name: 'down' }), 70)
		setTimeout(() => press(input, '\r', { name: 'return' }), 80)
		setTimeout(() => press(input, 'q', { name: 'q' }), 90)

		await expect(session).resolves.toBe(0)
		expect(output.buffer).toContain('kuizer fix --write')
		expect(output.buffer).toContain('kuizer fix --write --no-backup')
	})

	it('renders the restore editor with snapshot support', async () => {
		const input = new TestInput()
		const output = new TestOutput()

		const session = runMainMenu({
			input: input as unknown as NodeJS.ReadStream,
			output: output as unknown as NodeJS.WriteStream,
			config: TEST_CONFIG,
			version: '0.1.1',
		})

		setTimeout(() => press(input, '', { name: 'down' }), 0)
		setTimeout(() => press(input, '', { name: 'down' }), 10)
		setTimeout(() => press(input, '\r', { name: 'return' }), 20)
		setTimeout(() => press(input, 'q', { name: 'q' }), 30)

		await expect(session).resolves.toBe(0)
		expect(output.buffer).toContain('Current Command: kuizer restore')
		expect(output.buffer).toContain('Snapshot')
	})
})
