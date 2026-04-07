import { isAbsolute, relative } from 'node:path'
import { write, badge, bold, dim, rule, indent } from '../output/index.js'
import type { AnalysisBoundary, Finding } from '../types.js'
import type { Report } from './types.js'

/**
 * Groups findings by file path.
 *
 * @param findings - List of findings.
 * @returns Map of file → findings.
 */
function groupByFile(findings: readonly Finding[]): Map<string, Finding[]> {
	return findings.reduce((acc, f) => {
		const group = acc.get(f.file) ?? []
		group.push(f)
		return acc.set(f.file, group)
	}, new Map<string, Finding[]>())
}

function groupBoundariesByFile(
	boundaries: readonly AnalysisBoundary[]
): Map<string, AnalysisBoundary[]> {
	return boundaries.reduce((acc, boundary) => {
		const group = acc.get(boundary.file) ?? []
		group.push(boundary)
		return acc.set(boundary.file, group)
	}, new Map<string, AnalysisBoundary[]>())
}

function displayPath(file: string, cwd = process.cwd()): string {
	if (!isAbsolute(file)) {
		return file
	}

	const relativePath = relative(cwd, file)
	if (
		relativePath === '' ||
		relativePath === '..' ||
		relativePath.startsWith('../') ||
		relativePath.startsWith('..\\') ||
		isAbsolute(relativePath)
	) {
		return file
	}

	return relativePath
}

function formatLocation(line: number, col: number): string {
	return `L${line}:C${col}`
}

/**
 * Builds a summary line for the report.
 *
 * @param findings - List of findings.
 * @param durationMs - Execution duration in milliseconds.
 */
function summary(
	findings: readonly Finding[],
	durationMs: number,
	analysisBoundaries: readonly AnalysisBoundary[]
): string {
	const errors = findings.filter((f) => f.severity === 'error').length
	const warnings = findings.filter((f) => f.severity === 'warn').length
	const seconds = `${(durationMs / 1000).toFixed(1)}s`

	return dim(
		`${errors} errors · ${warnings} warnings · ${analysisBoundaries.length} boundaries · ${seconds}`
	)
}

/**
 * Pretty CLI reporter.
 *
 * @remarks
 * Outputs grouped, human-readable findings to stdout.
 *
 * @param findings - The list of findings to report.
 * @param durationMs - Execution duration in milliseconds.
 */
export function terminal({ findings, durationMs, analysisBoundaries }: Report): void {
	if (findings.length === 0) {
		write(`\n${dim('No findings.')}`)
		if (analysisBoundaries.length > 0) {
			write(dim(`Analysis boundaries: ${analysisBoundaries.length}`))
		}
		write(rule())
		return
	}

	for (const [file, group] of groupByFile(findings)) {
		write(`\n${bold(displayPath(file))}`)

		for (const f of group) {
			const loc = dim(formatLocation(f.line, f.col))
			const id = dim(f.detectorId)

			write(indent(`${badge(f.severity)}  ${loc}  ${id}  ${f.message}`))
		}
	}

	if (analysisBoundaries.length > 0) {
		write(`\n${bold('Analysis boundaries')}`)

		for (const [file, group] of groupBoundariesByFile(analysisBoundaries)) {
			write(`\n${bold(displayPath(file))}`)

			for (const boundary of group) {
				const loc = dim(formatLocation(boundary.line, boundary.col))
				const kind = dim(boundary.kind)
				write(indent(`${badge('info')}  ${loc}  ${kind}  ${boundary.message}`))
			}
		}
	}

	write(`\n${rule()}`)
	write(summary(findings, durationMs, analysisBoundaries))
}
