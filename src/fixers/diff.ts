import { existsSync, readFileSync } from 'node:fs'
import { basename } from 'node:path'
import type { FixPlan, PlannedEdit } from './types.js'

export function renderPlanDiff(plan: FixPlan): string {
	return plan.edits.map((edit) => renderEditDiff(edit)).join('\n')
}

function renderEditDiff(edit: PlannedEdit): string {
	switch (edit.kind) {
		case 'delete-file':
			return renderDeleteFileDiff(edit.file)

		case 'remove-export-keyword':
			return renderRemoveExportDiff(edit.file, edit.startLine, edit.endLine)

		case 'delete-declaration':
			return renderDeleteDeclarationDiff(edit.file, edit.startLine, edit.endLine)
	}
}

function renderDeleteFileDiff(file: string): string {
	const header = [`--- ${file}`, `+++ (deleted)`, `@@ delete-file @@`]
	if (!existsSync(file)) {
		return [...header, `(missing) ${basename(file)}`].join('\n')
	}

	const contents = readFileSync(file, 'utf8')
		.split('\n')
		.map((line) => `- ${line}`)
		.join('\n')

	return [...header, contents].join('\n')
}

function renderRemoveExportDiff(file: string, startLine: number, endLine: number): string {
	if (!existsSync(file)) {
		return `(missing) ${basename(file)}`
	}

	const lines = readFileSync(file, 'utf8').split('\n')
	const header = [`--- ${file}`, `+++ ${file}`, `@@ -${startLine},${endLine - startLine + 1} @@`]
	const diff: string[] = []

	for (let i = startLine - 1; i < endLine && i < lines.length; i++) {
		const original = lines[i]
		const stripped = original
			.replace(/^(\s*)export\s+default\s+/, '$1')
			.replace(/^(\s*)export\s+/, '$1')
		diff.push(`- ${original}`)
		diff.push(`+ ${stripped}`)
	}

	return [...header, ...diff].join('\n')
}

function renderDeleteDeclarationDiff(file: string, startLine: number, endLine: number): string {
	if (!existsSync(file)) {
		return `(missing) ${basename(file)}`
	}

	const lines = readFileSync(file, 'utf8').split('\n')
	const header = [`--- ${file}`, `+++ ${file}`, `@@ -${startLine},${endLine - startLine + 1} @@`]
	const diff: string[] = []

	for (let i = startLine - 1; i < endLine && i < lines.length; i++) {
		diff.push(`- ${lines[i]}`)
	}

	return [...header, ...diff].join('\n')
}
