import { describe, expect, it, spyOn } from 'bun:test'
import { createProgress } from '../../src/output/progress.js'

describe('progress', () => {
	it('is a no-op when stdout is not a tty', () => {
		let output = ''
		const progress = createProgress({
			stream: {
				isTTY: false,
				columns: 80,
				write(chunk: string) {
					output += chunk
					return true
				},
			},
		})

		progress.update('Loading')
		progress.stop()

		expect(output).toBe('')
	})

	it('renders updates and clears the status line', () => {
		const scheduled: (() => void)[] = []
		let output = ''
		const setIntervalSpy = spyOn(globalThis, 'setInterval').mockImplementation((fn: TimerHandler) => {
			scheduled.push(fn as () => void)
			return 1 as unknown as ReturnType<typeof setInterval>
		})

		const progress = createProgress({
			cwd: '/repo',
			stream: {
				isTTY: true,
				columns: 120,
				write(chunk: string) {
					output += chunk
					return true
				},
			},
		})

		progress.update('Analyzing import graph 3/10', '/repo/src/lib/util.ts')
		scheduled[0]?.()
		progress.stop()

		expect(output).toContain('\r- Working')
		expect(output).toContain('Analyzing import graph 3/10 src/lib/util.ts')
		expect(output).toContain('\r')
		setIntervalSpy.mockRestore()
	})
})
