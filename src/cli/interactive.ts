import { relative } from 'node:path'
import readline from 'node:readline'
import { restoreSnapshot } from '../backup/restore.js'
import { applyFixPlans } from '../fixers/apply.js'
import type { FixPlan } from '../fixers/types.js'
import type { Config, Finding } from '../types.js'

type InteractivePane = 'detectors' | 'files' | 'findings'
type Keypress = Readonly<{
	name?: string
	sequence: string
	ctrl: boolean
}>

type FileGroup = Readonly<{
	file: string
	findings: readonly Finding[]
	fixableKeys: readonly string[]
	selectedCount: number
}>

type DetectorCatalogItem = Readonly<{
	id: string
	label: string
	count: number
}>

type InteractiveViewport = Readonly<{
	columns: number
	rows: number
}>

export type InteractiveState = Readonly<{
	pane: InteractivePane
	fileCursor: number
	findingCursor: number
	detectorCursor: number
	detectorSelection: ReadonlySet<string>
	activeKinds: readonly string[]
	activeDetectors: readonly string[]
	selectedKeys: ReadonlySet<string>
	showHelp: boolean
	status: string
	lastSnapshotPath?: string
}>

type InteractiveModel = Readonly<{
	filteredFindings: readonly Finding[]
	filteredFixableCount: number
	selectedCount: number
	detectorCatalog: readonly DetectorCatalogItem[]
	detectorFilterLabel: string
	currentFileDetectorSummary: string
	fileGroups: readonly FileGroup[]
	currentFile?: FileGroup
	currentFindings: readonly Finding[]
	visibleFiles: readonly FileGroup[]
	visibleFindings: readonly Finding[]
	fileWindowStart: number
	findingWindowStart: number
	bodyRows: number
	leftWidth: number
	rightWidth: number
	viewport: InteractiveViewport
	fixPlansByKey: ReadonlyMap<string, FixPlan>
}>

type InteractiveSessionOptions = Readonly<{
	version: string
	config: Config
	cwd: string
	findings: readonly Finding[]
	supportedFixes: readonly FixPlan[]
	initialKinds?: readonly string[]
	initialDetectors?: readonly string[]
	refresh(): Promise<{
		findings: readonly Finding[]
		supportedFixes: readonly FixPlan[]
	}>
	input?: NodeJS.ReadStream
	output?: NodeJS.WriteStream
}>

const MIN_COLUMNS = 72
const MIN_ROWS = 16

function sortFindings(a: Finding, b: Finding): number {
	return (
		a.line - b.line ||
		a.col - b.col ||
		(a.symbol ?? a.message).localeCompare(b.symbol ?? b.message)
	)
}

function clamp(value: number, min: number, max: number): number {
	return Math.max(min, Math.min(value, max))
}

function truncateEnd(value: string, max: number): string {
	if (max <= 0) return ''
	if (value.length <= max) return value
	if (max <= 3) return value.slice(0, max)
	return `${value.slice(0, max - 3)}...`
}

function truncateMiddle(value: string, max: number): string {
	if (max <= 0) return ''
	if (value.length <= max) return value
	if (max <= 3) return value.slice(0, max)

	const remaining = max - 3
	const head = Math.ceil(remaining / 2)
	const tail = Math.floor(remaining / 2)
	return `${value.slice(0, head)}...${value.slice(-tail)}`
}

function pad(value: string, width: number): string {
	if (value.length >= width) {
		return value.slice(0, width)
	}

	return value + ' '.repeat(width - value.length)
}

function normalizeKinds(kinds: readonly string[]): string[] {
	if (kinds.includes('all')) {
		return ['all']
	}

	return [...new Set(kinds)].sort()
}

function normalizeDetectors(detectors: readonly string[]): string[] {
	if (detectors.includes('all')) {
		return ['all']
	}

	return [...new Set(detectors)]
}

export function findingKey(finding: Finding): string {
	return [
		finding.detectorId,
		finding.file,
		finding.line,
		finding.col,
		finding.symbol ?? '',
		finding.kind ?? '',
		finding.message,
	].join('::')
}

export function createInteractiveState(
	initialKinds: readonly string[] = [],
	initialDetectors: readonly string[] = ['all']
): InteractiveState {
	return {
		pane: 'detectors',
		fileCursor: 0,
		findingCursor: 0,
		detectorCursor: 0,
		detectorSelection: new Set(initialDetectors.includes('all') ? [] : [...initialDetectors]),
		activeKinds: normalizeKinds(initialKinds),
		activeDetectors: initialDetectors.length === 0 ? ['all'] : [...initialDetectors],
		selectedKeys: new Set<string>(),
		showHelp: false,
		status: 'Select detectors to analyze. Space toggles, Enter to start, a for all, q to quit.',
	}
}

function detectorDisplayName(detectorId: string): string {
	return detectorId
		.replace(/([a-z0-9])([A-Z])/g, '$1 $2')
		.replace(/[_-]+/g, ' ')
		.trim()
		.toLowerCase()
}

function collectDetectorCatalog(findings: readonly Finding[]): DetectorCatalogItem[] {
	const catalog = new Map<string, DetectorCatalogItem>()

	for (const finding of findings) {
		const current = catalog.get(finding.detectorId)
		if (current) {
			catalog.set(finding.detectorId, {
				...current,
				count: current.count + 1,
			})
			continue
		}

		catalog.set(finding.detectorId, {
			id: finding.detectorId,
			label: detectorDisplayName(finding.detectorId),
			count: 1,
		})
	}

	return [...catalog.values()].sort(
		(a, b) => a.label.localeCompare(b.label) || a.id.localeCompare(b.id)
	)
}

function formatDetectorSelectionLabel(
	activeDetectors: readonly string[],
	catalog: readonly DetectorCatalogItem[]
): string {
	if (activeDetectors.length === 0 || activeDetectors.includes('all')) {
		return 'all'
	}

	const active = new Set(activeDetectors)
	return catalog
		.filter((item) => active.has(item.id))
		.map((item) => item.label)
		.join(', ')
}

function formatDetectorShortcutLabel(catalog: readonly DetectorCatalogItem[]): string {
	if (catalog.length === 0) {
		return 'no detectors'
	}

	const items = catalog.slice(0, 9).map((item, index) => `${index + 1} ${item.label}`)
	if (catalog.length > 9) {
		items.push(`0 ${catalog[9]?.label ?? 'detector 10'}`)
	}
	items.push('* all')
	return items.join('  ')
}

function summarizeDetectors(
	findings: readonly Finding[],
	catalog: readonly DetectorCatalogItem[]
): string {
	if (findings.length === 0) {
		return 'no findings'
	}

	const counts = new Map<string, number>()
	for (const finding of findings) {
		counts.set(finding.detectorId, (counts.get(finding.detectorId) ?? 0) + 1)
	}

	const summary = catalog
		.map((item) => [item.label, counts.get(item.id) ?? 0] as const)
		.filter(([, count]) => count > 0)
		.map(([label, count]) => `${label}: ${count}`)

	return summary.length > 0 ? summary.join(', ') : 'no findings'
}

function applyDetectorFilters(
	selectedDetectors: readonly string[],
	allFindings: readonly Finding[]
): Finding[] {
	if (selectedDetectors.length === 0 || selectedDetectors.includes('all')) {
		return [...allFindings]
	}

	const allowed = new Set(selectedDetectors)
	return allFindings.filter((finding) => allowed.has(finding.detectorId))
}

function applyKindFilters(
	selectedKinds: readonly string[],
	findings: readonly Finding[]
): Finding[] {
	if (selectedKinds.length === 0 || selectedKinds.includes('all')) {
		return [...findings]
	}

	return findings.filter((finding) => {
		if (finding.kind === undefined) {
			return true
		}

		return selectedKinds.includes(finding.kind)
	})
}

function applyInteractiveFilters(
	selectedKinds: readonly string[],
	selectedDetectors: readonly string[],
	allFindings: readonly Finding[]
): Finding[] {
	return applyKindFilters(selectedKinds, applyDetectorFilters(selectedDetectors, allFindings))
}

function createViewport(stream: NodeJS.WriteStream): InteractiveViewport {
	return {
		columns: Math.max(MIN_COLUMNS, stream.columns ?? MIN_COLUMNS),
		rows: Math.max(MIN_ROWS, stream.rows ?? MIN_ROWS),
	}
}

function createWindowStart(total: number, cursor: number, size: number): number {
	if (total <= size) return 0

	const centered = cursor - Math.floor(size / 2)
	return clamp(centered, 0, total - size)
}

export function buildInteractiveModel(
	findings: readonly Finding[],
	supportedFixes: readonly FixPlan[],
	state: InteractiveState,
	viewport: InteractiveViewport
): InteractiveModel {
	const fixPlansByKey = new Map<string, FixPlan>(
		supportedFixes.map((plan) => [findingKey(plan.finding), plan])
	)
	const detectorCatalog = collectDetectorCatalog(findings)
	const filteredFindings = applyInteractiveFilters(
		state.activeKinds,
		state.activeDetectors,
		findings
	)

	const filteredFixableKeys = filteredFindings
		.map((finding) => findingKey(finding))
		.filter((key) => fixPlansByKey.has(key))
	const selectedCount = [...state.selectedKeys].filter((key) => fixPlansByKey.has(key)).length

	const grouped = filteredFindings.reduce((acc, finding) => {
		const current = acc.get(finding.file) ?? []
		current.push(finding)
		acc.set(finding.file, current)
		return acc
	}, new Map<string, Finding[]>())

	const fileGroups = [...grouped.entries()]
		.map(([file, fileFindings]) => {
			const sorted = [...fileFindings].sort(sortFindings)
			const fixableKeys = sorted
				.map((finding) => findingKey(finding))
				.filter((key) => fixPlansByKey.has(key))
			const selectedInFile = fixableKeys.filter((key) => state.selectedKeys.has(key)).length

			return {
				file,
				findings: sorted,
				fixableKeys,
				selectedCount: selectedInFile,
			} satisfies FileGroup
		})
		.sort((a, b) => b.findings.length - a.findings.length || a.file.localeCompare(b.file))

	const footerHeight = state.showHelp ? 3 : 2
	const bodyRows = Math.max(4, viewport.rows - 3 - 4 - footerHeight)
	const paneInnerWidth = Math.max(20, Math.floor((viewport.columns - 3) / 2))
	const leftWidth = paneInnerWidth
	const rightWidth = Math.max(20, viewport.columns - 3 - leftWidth)

	const fileCursor =
		fileGroups.length === 0 ? 0 : clamp(state.fileCursor, 0, fileGroups.length - 1)
	const currentFile = fileGroups[fileCursor]
	const currentFindings = currentFile?.findings ?? []
	const findingCursor =
		currentFindings.length === 0 ? 0 : clamp(state.findingCursor, 0, currentFindings.length - 1)

	const fileWindowStart = createWindowStart(fileGroups.length, fileCursor, bodyRows)
	const findingWindowStart = createWindowStart(currentFindings.length, findingCursor, bodyRows)

	return {
		filteredFindings,
		filteredFixableCount: filteredFixableKeys.length,
		selectedCount,
		detectorCatalog,
		detectorFilterLabel: formatDetectorSelectionLabel(state.activeDetectors, detectorCatalog),
		currentFileDetectorSummary: summarizeDetectors(currentFindings, detectorCatalog),
		fileGroups,
		currentFindings,
		visibleFiles: fileGroups.slice(fileWindowStart, fileWindowStart + bodyRows),
		visibleFindings: currentFindings.slice(findingWindowStart, findingWindowStart + bodyRows),
		fileWindowStart,
		findingWindowStart,
		bodyRows,
		leftWidth,
		rightWidth,
		viewport,
		fixPlansByKey,
		...(currentFile ? { currentFile } : {}),
	}
}

function withState(
	state: InteractiveState,
	patch: Partial<Omit<InteractiveState, 'selectedKeys'>> & {
		selectedKeys?: ReadonlySet<string>
	}
): InteractiveState {
	return {
		...state,
		...patch,
		selectedKeys: patch.selectedKeys ?? state.selectedKeys,
	}
}

function toggleKind(activeKinds: readonly string[], kind: string): string[] {
	const next = new Set(activeKinds)
	next.delete('all')

	if (next.has(kind)) {
		next.delete(kind)
	} else {
		next.add(kind)
	}

	return normalizeKinds([...next])
}

function toggleDetector(activeDetectors: readonly string[], detectorId: string): string[] {
	const next = new Set(activeDetectors)
	next.delete('all')

	if (next.has(detectorId)) {
		next.delete(detectorId)
	} else {
		next.add(detectorId)
	}

	return normalizeDetectors([...next])
}

function detectorIndexFromKey(key: string): number | undefined {
	if (key === '0') {
		return 9
	}

	if (/^[1-9]$/.test(key)) {
		return Number(key) - 1
	}

	return undefined
}

function resetDetectorSelection(
	selectedDetectors: readonly string[],
	availableFindings: readonly Finding[]
): string[] {
	const available = new Set(availableFindings.map((finding) => finding.detectorId))
	const next = [...selectedDetectors].filter((detectorId) => available.has(detectorId))
	return next.length > 0 ? normalizeDetectors(next) : ['all']
}

function moveInteractiveCursor(
	state: InteractiveState,
	model: InteractiveModel,
	delta: number
): InteractiveState {
	if (state.pane === 'detectors') {
		return state
	}

	if (state.pane === 'files') {
		return withState(state, {
			fileCursor: clamp(
				state.fileCursor + delta,
				0,
				Math.max(0, model.fileGroups.length - 1)
			),
			findingCursor: 0,
		})
	}

	return withState(state, {
		findingCursor: clamp(
			state.findingCursor + delta,
			0,
			Math.max(0, model.currentFindings.length - 1)
		),
	})
}

function switchInteractivePane(state: InteractiveState): InteractiveState {
	if (state.pane === 'detectors') {
		return withState(state, { pane: 'files' })
	}
	if (state.pane === 'files') {
		return withState(state, { pane: 'findings' })
	}
	return withState(state, { pane: 'files' })
}

/**
 * Toggles the selection of the current item in the interactive UI.
 *
 * @description When in the files pane, selects all fixable findings in the current file.
 * When in the findings pane, selects the current finding if it is fixable.
 */
export function toggleInteractiveSelection(
	state: InteractiveState,
	model: InteractiveModel
): InteractiveState {
	const nextSelected = new Set(state.selectedKeys)

	if (state.pane === 'files') {
		const fileGroup = model.currentFile
		if (!fileGroup || fileGroup.fixableKeys.length === 0) {
			return withState(state, {
				status: 'The current file has no fixable findings.',
			})
		}

		const allSelected = fileGroup.fixableKeys.every((key) => nextSelected.has(key))
		for (const key of fileGroup.fixableKeys) {
			if (allSelected) {
				nextSelected.delete(key)
			} else {
				nextSelected.add(key)
			}
		}

		return withState(state, {
			selectedKeys: nextSelected,
			status: allSelected
				? 'Removed the current file from the selection.'
				: `Selected ${fileGroup.fixableKeys.length} fixable finding(s) in the current file.`,
		})
	}

	const finding = model.currentFindings[state.findingCursor]
	if (!finding) {
		return withState(state, { status: 'There is no current finding to select.' })
	}

	const key = findingKey(finding)
	if (!model.fixPlansByKey.has(key)) {
		return withState(state, { status: 'The current finding is not fixable.' })
	}

	if (nextSelected.has(key)) {
		nextSelected.delete(key)
		return withState(state, {
			selectedKeys: nextSelected,
			status: `Removed '${finding.symbol ?? finding.message}' from the selection.`,
		})
	}

	nextSelected.add(key)
	return withState(state, {
		selectedKeys: nextSelected,
		status: `Selected '${finding.symbol ?? finding.message}'.`,
	})
}

function collectCurrentKeys(state: InteractiveState, model: InteractiveModel): string[] {
	if (state.pane === 'files') {
		return [...(model.currentFile?.fixableKeys ?? [])]
	}

	const currentFinding = model.currentFindings[state.findingCursor]
	if (!currentFinding) {
		return []
	}

	const key = findingKey(currentFinding)
	return model.fixPlansByKey.has(key) ? [key] : []
}

function collectSelectedKeys(state: InteractiveState, model: InteractiveModel): string[] {
	return [...state.selectedKeys].filter((key) => model.fixPlansByKey.has(key))
}

function collectAllFilteredKeys(model: InteractiveModel): string[] {
	return model.filteredFindings
		.map((finding) => findingKey(finding))
		.filter((key) => model.fixPlansByKey.has(key))
}

function kindToken(kind?: string): string {
	switch (kind) {
		case 'function':
			return 'fn'
		case 'const':
			return 'const'
		case 'type':
			return 'type'
		case 'interface':
			return 'iface'
		case 'class':
			return 'class'
		case 'enum':
			return 'enum'
		case 'default':
			return 'default'
		default:
			return '--'
	}
}

function fileRowLabel(
	group: FileGroup,
	index: number,
	state: InteractiveState,
	model: InteractiveModel,
	cwd: string
): string {
	const cursor =
		state.pane === 'files' && state.fileCursor === model.fileWindowStart + index ? '>' : ' '
	const selection = `[${group.selectedCount}/${group.fixableKeys.length}]`
	const relFile = relative(cwd, group.file) || group.file
	return `${cursor} ${selection} ${relFile}`
}

function findingRowLabel(
	finding: Finding,
	index: number,
	state: InteractiveState,
	model: InteractiveModel
): string {
	const absoluteIndex = model.findingWindowStart + index
	const cursor = state.pane === 'findings' && state.findingCursor === absoluteIndex ? '>' : ' '
	const key = findingKey(finding)
	const selectable = model.fixPlansByKey.has(key)
	const marker = state.selectedKeys.has(key) ? 'x' : selectable ? ' ' : '!'
	const symbol = finding.symbol ?? truncateEnd(finding.message, 28)
	return `${cursor} [${marker}] ${kindToken(finding.kind)} ${symbol} L${finding.line}`
}

function composePaneLine(
	left: string,
	right: string,
	leftWidth: number,
	rightWidth: number
): string {
	return `│${pad(left, leftWidth)}│${pad(right, rightWidth)}│`
}

export function renderInteractiveScreen(
	model: InteractiveModel,
	state: InteractiveState,
	version: string,
	cwd: string
): string {
	const { viewport, leftWidth, rightWidth } = model
	const fileHeaderBase = ` Files (${model.fileGroups.length})${state.pane === 'files' ? ' [active]' : ''}`
	const currentFileLabel = model.currentFile
		? truncateMiddle(
				relative(cwd, model.currentFile.file) || model.currentFile.file,
				rightWidth - 14
			)
		: 'no file selected'
	const findingHeaderBase = ` Findings (${model.currentFindings.length})${state.pane === 'findings' ? ' [active]' : ''}`
	const currentDetectorSummary = truncateEnd(model.currentFileDetectorSummary, rightWidth - 14)
	const filterLabel =
		state.activeKinds.length === 0 || state.activeKinds.includes('all')
			? 'all'
			: state.activeKinds.join(', ')
	const detectorLabel = truncateEnd(model.detectorFilterLabel, viewport.columns - 12)

	const lines = [
		pad(`kuizer ${version}  detect --interactive`, viewport.columns),
		pad(
			`${model.filteredFindings.length} findings  ${model.filteredFixableCount} fixable  ${model.selectedCount} selected  detectors: ${detectorLabel}  kinds: ${filterLabel}`,
			viewport.columns
		),
		pad(truncateEnd(state.status, viewport.columns), viewport.columns),
		`┌${'─'.repeat(leftWidth)}┬${'─'.repeat(rightWidth)}┐`,
		composePaneLine(
			truncateEnd(fileHeaderBase, leftWidth),
			truncateEnd(
				`${findingHeaderBase}  ${currentFileLabel}  ${currentDetectorSummary}`,
				rightWidth
			),
			leftWidth,
			rightWidth
		),
		`├${'─'.repeat(leftWidth)}┼${'─'.repeat(rightWidth)}┤`,
	]

	for (let index = 0; index < model.bodyRows; index++) {
		const file = model.visibleFiles[index]
		const finding = model.visibleFindings[index]
		const fileLabel = file
			? truncateMiddle(fileRowLabel(file, index, state, model, cwd), leftWidth)
			: ''
		const findingLabel = finding
			? truncateEnd(findingRowLabel(finding, index, state, model), rightWidth)
			: ''
		lines.push(composePaneLine(fileLabel, findingLabel, leftWidth, rightWidth))
	}

	lines.push(`└${'─'.repeat(leftWidth)}┴${'─'.repeat(rightWidth)}┘`)
	lines.push(
		pad(
			'Keys: ↑↓ move  Tab switch pane  Space select  Enter apply current  x apply selected  A apply all',
			viewport.columns
		)
	)
	lines.push(
		pad(
			state.showHelp
				? 'Files: Space/Enter works on all fixable findings in the file. Findings: Space/Enter works on one finding. Kinds: f c t i, a all, r reset. Detectors: 1-9 toggle, 0 toggle 10th, * all, d detector selection, q quit, R revert.'
				: `Keys: f function  c const  t type  i interface  a all  r reset  d detectors  |  Detectors: ${formatDetectorShortcutLabel(model.detectorCatalog)}  |  q quit  R revert`,
			viewport.columns
		)
	)

	if (state.showHelp) {
		lines.push(
			pad(
				'Markers: [x] selected fixable  [ ] fixable  [!] not fixable. The right pane follows the file selected in the left pane.',
				viewport.columns
			)
		)
	}

	return lines.join('\n')
}

export function renderDetectorSelectionScreen(
	detectorCatalog: readonly DetectorCatalogItem[],
	state: InteractiveState,
	viewport: InteractiveViewport
): string {
	const header = pad('Select Detectors to Analyze', viewport.columns)
	const subheader = pad(
		state.detectorSelection.size === 0
			? 'None selected (will search all)'
			: `${state.detectorSelection.size} selected`,
		viewport.columns
	)
	const statusLine = pad(truncateEnd(state.status, viewport.columns), viewport.columns)

	const bodyRows = Math.max(4, viewport.rows - 6)
	const catalog = [...detectorCatalog].sort((a, b) => a.label.localeCompare(b.label))
	const visibleCatalog = catalog.slice(0, bodyRows)

	const lines = [header, subheader, statusLine, `┌${'─'.repeat(viewport.columns - 2)}┐`]

	for (let index = 0; index < bodyRows; index++) {
		const detector = visibleCatalog[index]
		const cursor = state.detectorCursor === index && state.pane === 'detectors' ? '>' : ' '
		if (detector) {
			const isSelected =
				state.detectorSelection.has(detector.id) || state.detectorSelection.size === 0
			const marker = isSelected ? '[x]' : '[ ]'
			const line = `${cursor} ${marker} ${detector.label} (${detector.count})`
			lines.push(pad(truncateEnd(line, viewport.columns - 2), viewport.columns - 2) + '│')
		} else {
			lines.push(pad('', viewport.columns - 2) + '│')
		}
	}

	lines.push(`└${'─'.repeat(viewport.columns - 2)}┘`)
	lines.push(pad('Space toggles  a all  none  Enter to start  q quit', viewport.columns))

	return lines.join('\n')
}

function readKeypress(stream: NodeJS.ReadStream): Promise<Keypress> {
	return new Promise((resolve) => {
		const onKeypress = (
			sequence: string,
			key: { ctrl?: boolean; name?: string } | undefined
		) => {
			stream.off('keypress', onKeypress)
			resolve({
				sequence,
				ctrl: key?.ctrl === true,
				...(key?.name ? { name: key.name } : {}),
			})
		}

		stream.on('keypress', onKeypress)
	})
}

function applyKeys(
	keys: readonly string[],
	model: InteractiveModel,
	config: Config
): { appliedCount: number; message: string; snapshotPath?: string } {
	if (keys.length === 0) {
		return {
			appliedCount: 0,
			message: 'Nothing to apply for the current action.',
		}
	}

	const plans = keys
		.map((key) => model.fixPlansByKey.get(key))
		.filter((plan): plan is FixPlan => plan !== undefined)

	if (plans.length === 0) {
		return {
			appliedCount: 0,
			message: 'Nothing to apply for the current action.',
		}
	}

	const result = applyFixPlans(plans, [], config, { backup: true })
	const snapshotSuffix = result.snapshotPath ? ` Snapshot: ${result.snapshotPath}` : ''

	return {
		appliedCount: result.applied.length,
		...(result.snapshotPath && { snapshotPath: result.snapshotPath }),
		message: `Applied ${result.applied.length} fix(es).${snapshotSuffix}`,
	}
}

function resetSelection(
	selectedKeys: ReadonlySet<string>,
	availableFindings: readonly Finding[]
): Set<string> {
	const available = new Set(availableFindings.map((finding) => findingKey(finding)))
	return new Set([...selectedKeys].filter((key) => available.has(key)))
}

export async function runInteractiveSession(
	options: InteractiveSessionOptions
): Promise<readonly Finding[]> {
	const input = options.input ?? process.stdin
	const output = options.output ?? process.stdout

	if (!input.isTTY || !output.isTTY || typeof input.setRawMode !== 'function') {
		throw new Error('--interactive requires a TTY on stdin/stdout')
	}

	let findings = [...options.findings]
	let supportedFixes = [...options.supportedFixes]
	let state = createInteractiveState(options.initialKinds, options.initialDetectors)

	readline.emitKeypressEvents(input)
	input.setRawMode(true)
	input.resume()
	output.write('\x1b[?25l')

	try {
		let running = true
		const viewport = createViewport(output)

		while (running) {
			output.write('\x1b[2J\x1b[H')

			if (state.pane === 'detectors') {
				const catalog = collectDetectorCatalog(findings)
				output.write(renderDetectorSelectionScreen(catalog, state, viewport))
			} else {
				const currentModel = buildInteractiveModel(
					findings,
					supportedFixes,
					state,
					viewport
				)
				output.write(
					renderInteractiveScreen(currentModel, state, options.version, options.cwd)
				)
			}

			const key = await readKeypress(input)

			if (key.ctrl && key.name === 'c') {
				running = false
				continue
			}

			if (state.pane === 'detectors') {
				switch (key.name ?? key.sequence) {
					case 'up':
						state = withState(state, {
							detectorCursor: Math.max(0, state.detectorCursor - 1),
						})
						break
					case 'down': {
						const catalog = collectDetectorCatalog(findings)
						state = withState(state, {
							detectorCursor: Math.min(catalog.length - 1, state.detectorCursor + 1),
						})
						break
					}
					case 'pageup':
						state = withState(state, { detectorCursor: 0 })
						break
					case 'pagedown': {
						const catalog = collectDetectorCatalog(findings)
						state = withState(state, { detectorCursor: catalog.length - 1 })
						break
					}
					case ' ': {
						const catalog = [...findings]
							.map((f) => f.detectorId)
							.filter((id, i, arr) => arr.indexOf(id) === i)
							.sort()
						const uniqueDetectors = catalog.map((id) => ({
							id,
							label: detectorDisplayName(id),
						}))
						const detector = uniqueDetectors[state.detectorCursor]
						if (detector) {
							const nextSelection = new Set(state.detectorSelection)
							if (nextSelection.has(detector.id)) {
								nextSelection.delete(detector.id)
							} else {
								nextSelection.add(detector.id)
							}
							state = withState(state, {
								detectorSelection: nextSelection,
								status: `Toggled ${detector.label}.`,
							})
						}
						break
					}
					case 'a':
						state = withState(state, {
							detectorSelection: new Set<string>(),
							status: 'All detectors selected.',
						})
						break
					case 'n': {
						const catalog = [...findings]
							.map((f) => f.detectorId)
							.filter((id, i, arr) => arr.indexOf(id) === i)
							.sort()
						const allDetectors = new Set(catalog)
						state = withState(state, {
							detectorSelection: allDetectors,
							status: 'All detectors selected.',
						})
						break
					}
					case 'return':
					case '\r': {
						const activeDetectors =
							state.detectorSelection.size === 0
								? ['all']
								: [...state.detectorSelection]
						const filteredFindings = applyDetectorFilters(activeDetectors, findings)
						const filteredFixes = supportedFixes.filter((plan) =>
							filteredFindings.some((f) => findingKey(f) === findingKey(plan.finding))
						)
						findings = [...filteredFindings]
						supportedFixes = [...filteredFixes]
						state = withState(state, {
							pane: 'files',
							activeDetectors,
							fileCursor: 0,
							findingCursor: 0,
							status: `Selected ${activeDetectors.includes('all') ? 'all detectors' : activeDetectors.length + ' detectors'}.`,
						})
						break
					}
					case 'tab':
						state = switchInteractivePane(state)
						break
					case 'q':
					case 'escape':
						running = false
						break
				}
				continue
			}

			const currentModel = buildInteractiveModel(findings, supportedFixes, state, viewport)

			switch (key.name ?? key.sequence) {
				case 'up':
					state = moveInteractiveCursor(state, currentModel, -1)
					break
				case 'down':
					state = moveInteractiveCursor(state, currentModel, 1)
					break
				case 'pageup':
					state = moveInteractiveCursor(
						state,
						currentModel,
						-Math.max(1, currentModel.bodyRows - 1)
					)
					break
				case 'pagedown':
					state = moveInteractiveCursor(
						state,
						currentModel,
						Math.max(1, currentModel.bodyRows - 1)
					)
					break
				case 'tab':
					state = switchInteractivePane(state)
					break
				case 'f':
					state = withState(state, {
						activeKinds: toggleKind(state.activeKinds, 'function'),
						fileCursor: 0,
						findingCursor: 0,
						status: 'Toggled the function filter.',
					})
					break
				case 'c':
					state = withState(state, {
						activeKinds: toggleKind(state.activeKinds, 'const'),
						fileCursor: 0,
						findingCursor: 0,
						status: 'Toggled the const filter.',
					})
					break
				case 't':
					state = withState(state, {
						activeKinds: toggleKind(state.activeKinds, 'type'),
						fileCursor: 0,
						findingCursor: 0,
						status: 'Toggled the type filter.',
					})
					break
				case 'i':
					state = withState(state, {
						activeKinds: toggleKind(state.activeKinds, 'interface'),
						fileCursor: 0,
						findingCursor: 0,
						status: 'Toggled the interface filter.',
					})
					break
				case 'a':
					state = withState(state, {
						activeKinds: ['all'],
						fileCursor: 0,
						findingCursor: 0,
						status: 'Showing all finding kinds.',
					})
					break
				case '*':
					state = withState(state, {
						activeDetectors: ['all'],
						fileCursor: 0,
						findingCursor: 0,
						status: 'Showing all detector categories.',
					})
					break
				case '0':
				case '1':
				case '2':
				case '3':
				case '4':
				case '5':
				case '6':
				case '7':
				case '8':
				case '9': {
					const detectorIndex = detectorIndexFromKey(key.name ?? key.sequence)
					const detector = currentModel.detectorCatalog[detectorIndex ?? -1]
					if (!detector) {
						state = withState(state, { status: 'No detector is assigned to that key.' })
						break
					}

					state = withState(state, {
						activeDetectors: toggleDetector(state.activeDetectors, detector.id),
						fileCursor: 0,
						findingCursor: 0,
						status: `Toggled the ${detector.label} detector filter.`,
					})
					break
				}
				case 'r':
					state = withState(state, {
						activeKinds: ['all'],
						activeDetectors: ['all'],
						fileCursor: 0,
						findingCursor: 0,
						status: 'Reset the interactive filters.',
					})
					break
				case 'd':
					state = withState(state, {
						pane: 'detectors',
						detectorCursor: 0,
						detectorSelection: new Set(
							state.activeDetectors.includes('all') ? [] : [...state.activeDetectors]
						),
						status: 'Select detectors to analyze. Space toggles, Enter to start.',
					})
					break
				case '?':
					state = withState(state, {
						showHelp: !state.showHelp,
						status: state.showHelp ? 'Collapsed help.' : 'Expanded help.',
					})
					break
				case ' ':
					state = toggleInteractiveSelection(state, currentModel)
					break
				case 'return':
				case '\r': {
					const keys = collectCurrentKeys(state, currentModel)
					const result = applyKeys(keys, currentModel, options.config)
					if (result.appliedCount === 0) {
						state = withState(state, { status: result.message })
						break
					}

					const refreshed = await options.refresh()
					findings = [...refreshed.findings]
					supportedFixes = [...refreshed.supportedFixes]
					state = withState(state, {
						activeDetectors: resetDetectorSelection(state.activeDetectors, findings),
						selectedKeys: resetSelection(state.selectedKeys, findings),
						status: result.message,
						...(result.snapshotPath && { lastSnapshotPath: result.snapshotPath }),
					})
					break
				}
				case 'x': {
					const keys = collectSelectedKeys(state, currentModel)
					const result = applyKeys(keys, currentModel, options.config)
					if (result.appliedCount === 0) {
						state = withState(state, {
							status: 'No selected fixable findings to apply.',
						})
						break
					}

					const refreshed = await options.refresh()
					findings = [...refreshed.findings]
					supportedFixes = [...refreshed.supportedFixes]
					state = withState(state, {
						activeDetectors: resetDetectorSelection(state.activeDetectors, findings),
						selectedKeys: resetSelection(state.selectedKeys, findings),
						status: result.message,
						...(result.snapshotPath && { lastSnapshotPath: result.snapshotPath }),
					})
					break
				}
				case 'A': {
					const keys = collectAllFilteredKeys(currentModel)
					const result = applyKeys(keys, currentModel, options.config)
					if (result.appliedCount === 0) {
						state = withState(state, {
							status: 'No filtered fixable findings to apply.',
						})
						break
					}

					const refreshed = await options.refresh()
					findings = [...refreshed.findings]
					supportedFixes = [...refreshed.supportedFixes]
					state = withState(state, {
						activeDetectors: resetDetectorSelection(state.activeDetectors, findings),
						selectedKeys: resetSelection(state.selectedKeys, findings),
						status: result.message,
						...(result.snapshotPath && { lastSnapshotPath: result.snapshotPath }),
					})
					break
				}
				case 'R': {
					if (!state.lastSnapshotPath) {
						state = withState(state, { status: 'No snapshot available to revert.' })
						break
					}

					try {
						const result = restoreSnapshot(state.lastSnapshotPath, {
							cwd: process.cwd(),
							outputDir: options.config.outputDir,
						})
						state = withState(state, {
							status: `Reverted ${result.restored.length} file${result.restored.length === 1 ? '' : 's'}`,
						})
						// Clear lastSnapshotPath by creating a new state without it
						const { lastSnapshotPath: _, ...stateWithoutSnapshot } = state
						state = stateWithoutSnapshot

						// Refresh findings after revert
						const refreshed = await options.refresh()
						findings = [...refreshed.findings]
						supportedFixes = [...refreshed.supportedFixes]
						state = withState(state, {
							activeDetectors: resetDetectorSelection(
								state.activeDetectors,
								findings
							),
							selectedKeys: resetSelection(state.selectedKeys, findings),
						})
					} catch (error) {
						state = withState(state, {
							status: `Failed to revert: ${error instanceof Error ? error.message : 'Unknown error'}`,
						})
					}
					break
				}
				case 'q':
				case 'escape':
					running = false
					break
			}
		}
	} finally {
		input.setRawMode(false)
		output.write('\x1b[?25h')
		output.write('\x1b[2J\x1b[H')
	}

	return findings
}
