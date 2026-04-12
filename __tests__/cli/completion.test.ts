import { describe, expect, it, spyOn } from 'bun:test'
import { runCompletion } from '../../src/cli/commands.js'

describe('completion command', () => {
	it('prints setup instructions when no shell is provided', async () => {
		const writeSpy = spyOn(process.stdout, 'write').mockImplementation(() => true)

		try {
			const exitCode = await runCompletion({ shell: undefined })
			const output = writeSpy.mock.calls.map((call) => String(call[0])).join('')

			expect(exitCode).toBe(0)
			expect(output).toContain('Shell completions')
			expect(output).toContain('kuizer completion --shell bash')
		} finally {
			writeSpy.mockRestore()
		}
	})

	it('prints a bash completion script', async () => {
		const writeSpy = spyOn(process.stdout, 'write').mockImplementation(() => true)

		try {
			const exitCode = await runCompletion({ shell: 'bash' })
			const output = writeSpy.mock.calls.map((call) => String(call[0])).join('')

			expect(exitCode).toBe(0)
			expect(output).toContain('complete -F _kuizer_completions kuizer')
		} finally {
			writeSpy.mockRestore()
		}
	})

	it('rejects unsupported shells', async () => {
		const errorSpy = spyOn(console, 'error').mockImplementation(() => {})

		try {
			const exitCode = await runCompletion({ shell: 'powershell' })

			expect(exitCode).toBe(1)
			expect(errorSpy).toHaveBeenCalled()
		} finally {
			errorSpy.mockRestore()
		}
	})
})
