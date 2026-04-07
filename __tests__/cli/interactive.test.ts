import { describe, expect, it } from 'bun:test'
import {
	buildInteractiveModel,
	createInteractiveState,
	findingKey,
	renderInteractiveScreen,
	toggleInteractiveSelection,
	type InteractiveState,
} from '../../src/cli/interactive.js'
import type { FixPlan } from '../../src/fixers/types.js'
import type { Finding } from '../../src/types.js'

const FILE_A = '/repo/src/a.ts'
const FILE_B = '/repo/src/b.ts'

function makeFinding(overrides: Partial<Finding> & Pick<Finding, 'file' | 'line' | 'message'>): Finding {
	return {
		detectorId: 'unusedExports',
		severity: 'warn',
		col: 1,
		symbol: 'symbol',
		kind: 'function',
		fixable: true,
		...overrides,
	}
}

function makeFixPlan(finding: Finding): FixPlan {
	return {
		supported: true,
		finding,
		edits: [
			{
				kind: 'delete-declaration',
				file: finding.file,
				exportName: finding.symbol ?? 'symbol',
				startLine: finding.line,
				endLine: finding.line,
				description: 'Delete declaration',
			},
		],
		safety: 'cautious',
		reason: 'Test fix',
	}
}

function buildState(overrides: Partial<InteractiveState> = {}): InteractiveState {
	return {
		...createInteractiveState(),
		...overrides,
	}
}

describe('interactive TUI helpers', () => {
	const findings = [
		makeFinding({ file: FILE_A, line: 5, symbol: 'alpha', message: 'alpha unused', kind: 'function' }),
		makeFinding({ file: FILE_A, line: 10, symbol: 'beta', message: 'beta unused', kind: 'type' }),
		makeFinding({ file: FILE_B, line: 3, symbol: 'gamma', message: 'gamma unused', kind: 'const' }),
		makeFinding({ file: FILE_B, line: 8, symbol: 'delta', message: 'delta unused', kind: 'interface' }),
		makeFinding({
			file: FILE_B,
			line: 14,
			symbol: 'epsilon',
			message: 'epsilon unused',
			kind: 'class',
			fixable: false,
		}),
	] satisfies Finding[]

	const fixPlans = findings
		.filter((finding) => finding.fixable)
		.map((finding) => makeFixPlan(finding))

	it('selects and deselects all fixable findings in the current file from the file pane', () => {
		const state = buildState({ pane: 'files' })
		const model = buildInteractiveModel(findings, fixPlans, state, { columns: 100, rows: 18 })

		const selected = toggleInteractiveSelection(state, model)
		expect([...selected.selectedKeys]).toEqual(
			model.currentFile?.fixableKeys ? [...model.currentFile.fixableKeys] : []
		)

		const cleared = toggleInteractiveSelection(selected, model)
		expect([...cleared.selectedKeys]).toEqual([])
	})

	it('does not select an unfixable finding from the findings pane', () => {
		const state = buildState({
			pane: 'findings',
			findingCursor: 2,
		})
		const model = buildInteractiveModel(findings, fixPlans, state, { columns: 100, rows: 18 })
		const next = toggleInteractiveSelection(state, model)

		expect(next.selectedKeys.size).toBe(0)
		expect(next.status).toContain('not fixable')
	})

	it('filters findings by detector selection and shows detector shortcuts', () => {
		const detectorFindings = [
			makeFinding({
				file: FILE_A,
				line: 5,
				symbol: 'alpha',
				message: 'alpha unused',
				kind: 'function',
				detectorId: 'unusedImports',
			}),
			makeFinding({
				file: FILE_A,
				line: 10,
				symbol: 'beta',
				message: 'beta unused',
				kind: 'function',
				detectorId: 'unusedComponents',
			}),
			makeFinding({
				file: FILE_B,
				line: 3,
				symbol: 'gamma',
				message: 'gamma unused',
				kind: 'type',
				detectorId: 'unusedTypes',
			}),
			makeFinding({
				file: FILE_B,
				line: 8,
				symbol: 'delta',
				message: 'delta unused',
				kind: 'const',
				detectorId: 'unusedImports',
			}),
		] satisfies Finding[]
		const detectorFixPlans = detectorFindings.filter((finding) => finding.fixable).map(makeFixPlan)
		const state = buildState({
			activeDetectors: ['unusedImports'],
		})

		const model = buildInteractiveModel(detectorFindings, detectorFixPlans, state, {
			columns: 180,
			rows: 18,
		})
		const screen = renderInteractiveScreen(model, state, '0.1.1', '/repo')

		expect(model.filteredFindings.map((finding) => finding.detectorId)).toEqual([
			'unusedImports',
			'unusedImports',
		])
		expect(model.detectorFilterLabel).toBe('unused imports')
		expect(screen).toContain('detectors: unused imports')
		expect(screen).toContain('Detectors: 1 unused components')
		expect(screen).toContain('2 unused imports')
		expect(screen).toContain('3 unused types')
		expect(screen).toContain('* all')
	})

	it('renders within the terminal height budget', () => {
		const state = buildState({
			selectedKeys: new Set([findingKey(findings[0])]),
		})
		const model = buildInteractiveModel(findings, fixPlans, state, { columns: 96, rows: 16 })
		const screen = renderInteractiveScreen(model, state, '0.1.1', '/repo')

		expect(screen.split('\n')).toHaveLength(16)
		expect(screen).toContain('detect --interactive')
		expect(screen).toContain('Files (2)')
		expect(screen).toContain('Findings (3)')
	})
})
