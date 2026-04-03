import { bold, dim, indent, rule, write } from '../output/index.js'
import { renderPlanDiff } from './diff.js'
import type { AppliedFixResult, FixPlanningResult } from './types.js'

export function reportFixPlan(result: FixPlanningResult, options: { dryRun: boolean }): void {
	if (result.supported.length === 0 && result.unsupported.length === 0) {
		write(dim('No fixable findings.'))
		write(rule())
		return
	}

	if (result.supported.length > 0) {
		write(bold(options.dryRun ? 'Dry-run fix plan' : 'Fix plan'))

		for (const plan of result.supported) {
			write(`\n${bold(plan.finding.file)}`)
			write(indent(`${plan.finding.detectorId}  ${plan.reason}`))
			write(indent(renderPlanDiff(plan)))
		}
	}

	if (result.unsupported.length > 0) {
		write(`\n${bold('Unsupported fixes')}`)
		for (const unsupported of result.unsupported) {
			write(indent(`${unsupported.finding.detectorId}  ${unsupported.finding.file}`))
			write(indent(dim(unsupported.reason), 4))
		}
	}

	write(`\n${rule()}`)
	write(dim(`${result.supported.length} supported · ${result.unsupported.length} unsupported`))
}

export function reportAppliedFixes(result: AppliedFixResult): void {
	write(bold('Applied fixes'))

	for (const plan of result.applied) {
		write(`\n${bold(plan.finding.file)}`)
		write(indent(`${plan.finding.detectorId}  ${plan.reason}`))
	}

	if (result.snapshotPath) {
		write(`\n${dim(`Snapshot: ${result.snapshotPath}`)}`)
	}

	if (result.skipped.length > 0) {
		write(`\n${bold('Skipped')}`)
		for (const skipped of result.skipped) {
			write(indent(`${skipped.finding.detectorId}  ${skipped.finding.file}`))
			write(indent(dim(skipped.reason), 4))
		}
	}

	write(`\n${rule()}`)
	write(dim(`${result.applied.length} applied · ${result.skipped.length} skipped`))
}
