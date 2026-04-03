import pc from 'picocolors'
import type { Severity } from '../types.js'

export function dim(s: string) {
	return pc.dim(s)
}

export function bold(s: string) {
	return pc.bold(s)
}

export function green(s: string) {
	return pc.green(s)
}

export function cyan(s: string) {
	return pc.cyan(s)
}

export function yellow(s: string) {
	return pc.yellow(s)
}

export function red(s: string) {
	return pc.red(s)
}

function colorize(s: string, severity: Severity) {
	if (severity === 'error') return pc.red(s)
	if (severity === 'warn') return pc.yellow(s)
	return pc.cyan(s)
}

export function badge(severity: Severity) {
	const label = severity.padEnd(5)
	return colorize(label, severity)
}

export function rule(width = 60) {
	return dim('─'.repeat(width))
}

export function indent(s: string, n = 2) {
	const pad = ' '.repeat(n)
	return s
		.split('\n')
		.map((line) => pad + line)
		.join('\n')
}

export function write(s: string) {
	process.stdout.write(s + '\n')
}
