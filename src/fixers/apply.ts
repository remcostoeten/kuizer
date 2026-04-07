import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import type { AppliedFixResult, FixPlan, PlannedEdit, UnsupportedFix } from './types.js'
import type { Config } from '../types.js'
import { createSnapshot } from '../backup/create.js'

export function applyFixPlans(
	plans: readonly FixPlan[],
	unsupported: readonly UnsupportedFix[],
	config: Config,
	options: { backup: boolean }
): AppliedFixResult {
	const allEdits = plans.flatMap((plan) => plan.edits)
	const snapshot =
		options.backup && allEdits.length > 0 ? createSnapshot(allEdits, config) : undefined

	for (const edit of orderEditsForApply(allEdits)) {
		applyEdit(edit)
	}

	return {
		applied: plans,
		skipped: unsupported,
		...(snapshot ? { snapshotPath: snapshot.manifestPath } : {}),
	}
}

function orderEditsForApply(edits: readonly PlannedEdit[]): PlannedEdit[] {
	const grouped = edits.reduce((acc, edit) => {
		const current = acc.get(edit.file) ?? []
		current.push(edit)
		acc.set(edit.file, current)
		return acc
	}, new Map<string, PlannedEdit[]>())

	return [...grouped.values()].flatMap((fileEdits) => {
		const deleteFile = fileEdits.find((edit) => edit.kind === 'delete-file')
		if (deleteFile) {
			return [deleteFile]
		}

		return [...fileEdits].sort((a, b) => {
			const startA = 'startLine' in a ? a.startLine : 0
			const startB = 'startLine' in b ? b.startLine : 0
			const endA = 'endLine' in a ? a.endLine : 0
			const endB = 'endLine' in b ? b.endLine : 0

			return endB - endA || startB - startA
		})
	})
}

function applyEdit(edit: PlannedEdit): void {
	switch (edit.kind) {
		case 'delete-file':
			if (existsSync(edit.file)) {
				rmSync(edit.file)
			}
			break

		case 'remove-export-keyword':
			applyRemoveExportKeyword(edit)
			break

		case 'delete-declaration':
			applyDeleteDeclaration(edit)
			break
	}
}

function applyRemoveExportKeyword(edit: {
	file: string
	startLine: number
	endLine: number
}): void {
	if (!existsSync(edit.file)) return

	const content = readFileSync(edit.file, 'utf8')
	const lines = content.split('\n')
	const result: string[] = []

	for (let i = 0; i < lines.length; i++) {
		const line = lines[i]
		if (line === undefined) {
			continue
		}

		const lineNum = i + 1
		if (lineNum >= edit.startLine && lineNum <= edit.endLine) {
			// Strip 'export default ', 'export ', from the line
			const stripped = line
				.replace(/^(\s*)export\s+default\s+/, '$1')
				.replace(/^(\s*)export\s+/, '$1')
			result.push(stripped)
		} else {
			result.push(line)
		}
	}

	writeFileSync(edit.file, result.join('\n'), 'utf8')
}

function applyDeleteDeclaration(edit: { file: string; startLine: number; endLine: number }): void {
	if (!existsSync(edit.file)) return

	const content = readFileSync(edit.file, 'utf8')
	const lines = content.split('\n')
	const result: string[] = []

	let prevWasBlank = false
	for (let i = 0; i < lines.length; i++) {
		const line = lines[i]
		if (line === undefined) {
			continue
		}

		const lineNum = i + 1
		if (lineNum >= edit.startLine && lineNum <= edit.endLine) {
			continue
		}

		const isBlank = line.trim() === ''
		// Skip consecutive blank lines that would result from removing declarations
		if (isBlank && prevWasBlank) continue
		prevWasBlank = isBlank
		result.push(line)
	}

	while (result[0] !== undefined && result[0].trim() === '') {
		result.shift()
	}

	// Trim trailing blank lines
	while (result.length > 0) {
		const lastLine = result.at(-1)
		if (lastLine === undefined || lastLine.trim() !== '') {
			break
		}

		result.pop()
	}
	// Ensure file ends with a newline
	result.push('')

	writeFileSync(edit.file, result.join('\n'), 'utf8')
}
