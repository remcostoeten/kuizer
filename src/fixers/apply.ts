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

	for (const plan of plans) {
		for (const edit of plan.edits) {
			applyEdit(edit)
		}
	}

	return {
		applied: plans,
		skipped: unsupported,
		...(snapshot ? { snapshotPath: snapshot.manifestPath } : {}),
	}
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
		const lineNum = i + 1
		if (lineNum >= edit.startLine && lineNum <= edit.endLine) {
			// Strip 'export default ', 'export ', from the line
			const stripped = lines[i]
				.replace(/^(\s*)export\s+default\s+/, '$1')
				.replace(/^(\s*)export\s+/, '$1')
			result.push(stripped)
		} else {
			result.push(lines[i])
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
		const lineNum = i + 1
		if (lineNum >= edit.startLine && lineNum <= edit.endLine) {
			continue
		}

		const isBlank = lines[i].trim() === ''
		// Skip consecutive blank lines that would result from removing declarations
		if (isBlank && prevWasBlank) continue
		prevWasBlank = isBlank
		result.push(lines[i])
	}

	// Trim trailing blank lines
	while (result.length > 0 && result[result.length - 1].trim() === '') {
		result.pop()
	}
	// Ensure file ends with a newline
	result.push('')

	writeFileSync(edit.file, result.join('\n'), 'utf8')
}
