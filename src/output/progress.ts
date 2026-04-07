import { isAbsolute, relative } from 'node:path'

type ProgressStream = Pick<NodeJS.WriteStream, 'write' | 'isTTY' | 'columns'>

type ProgressOptions = Readonly<{
	stream?: ProgressStream
	cwd?: string
	intervalMs?: number
}>

type ProgressHandle = Readonly<{
	update(label: string, detail?: string): void
	stop(finalMessage?: string): void
}>

const FRAMES = ['-', '\\', '|', '/'] as const

type LineState = Readonly<{
	label: string
	detail?: string
}>

function formatLine(frame: string, state: LineState, width: number | undefined): string {
	let suffix = ''
	if (state.detail) {
		suffix = ` ${state.detail}`
	}

	const raw = `${frame} ${state.label}${suffix}`
	if (!width || raw.length <= width) {
		return raw
	}

	if (!state.detail) {
		return raw.slice(0, Math.max(0, width - 1))
	}

	const available = Math.max(0, width - frame.length - state.label.length - 5)
	if (available <= 0) {
		return `${frame} ${state.label}`.slice(0, Math.max(0, width - 1))
	}

	return `${frame} ${state.label} ${truncate(state.detail, available)}`
}

function truncate(value: string, max: number): string {
	if (value.length <= max) {
		return value
	}

	if (max <= 3) {
		return value.slice(0, max)
	}

	return `...${value.slice(-(max - 3))}`
}

function normalizeDetail(cwd: string, nextDetail?: string): string | undefined {
	if (!nextDetail) {
		return undefined
	}

	if (!isAbsolute(nextDetail)) {
		return nextDetail
	}

	return relative(cwd, nextDetail)
}

function padLine(line: string, previousWidth: number): string {
	if (previousWidth > line.length) {
		return line.padEnd(previousWidth, ' ')
	}

	return line
}

function createNoopProgress(): ProgressHandle {
	return {
		update() {},
		stop() {},
	}
}

export function createProgress(options: ProgressOptions = {}): ProgressHandle {
	const stream = options.stream ?? process.stdout
	if (!stream.isTTY) {
		return createNoopProgress()
	}

	return createActiveProgress(stream, options.cwd ?? process.cwd(), options.intervalMs ?? 80)
}

function createActiveProgress(
	stream: ProgressStream,
	cwd: string,
	intervalMs: number
): ProgressHandle {
	let index = 0
	let state: LineState = { label: 'Working' }
	let lastWidth = 0
	let stopped = false

	const render = () => {
		if (stopped) return

		const frame = FRAMES[index % FRAMES.length] ?? FRAMES[0]
		const line = formatLine(frame, state, stream.columns)
		const padded = padLine(line, lastWidth)
		lastWidth = Math.max(lastWidth, line.length)
		stream.write(`\r${padded}`)
		index++
	}

	const timer = setInterval(render, intervalMs)
	render()

	return {
		update(nextLabel: string, nextDetail?: string) {
			const detail = normalizeDetail(cwd, nextDetail)
			state = {
				label: nextLabel,
				...(detail === undefined ? {} : { detail }),
			}
		},
		stop(finalMessage?: string) {
			if (stopped) return
			stopped = true
			clearInterval(timer)

			if (finalMessage) {
				const line = formatLine('o', { label: finalMessage }, stream.columns)
				stream.write(`\r${padLine(line, lastWidth)}\n`)
				return
			}

			stream.write(`\r${' '.repeat(lastWidth)}\r`)
		},
	}
}
