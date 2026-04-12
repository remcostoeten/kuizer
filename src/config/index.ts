import { existsSync } from 'node:fs'
import { homedir } from 'node:os'
import { basename, dirname, isAbsolute, resolve } from 'node:path'
import { cosmiconfig } from 'cosmiconfig'
import type { Config, Framework } from '../types.js'
import { detectTsConfig } from './detect-tsconfig.js'
import { defaults } from './defaults.js'
import { preset } from './presets.js'

type Raw = Partial<{
	tsconfig: string
	entrypoints: string[]
	framework: Framework
	countTestsAsConsumers: boolean
	allowedExports: string[]
	outputDir: string
	dryRunByDefault: boolean
	ignore: Partial<{
		files: string[]
		symbols: string[]
		paths: string[]
	}>
	detectors: Config['detectors']
}>

type SeverityValue = 'error' | 'warn' | 'info'

const VALID_FRAMEWORKS = new Set<Framework>(['none', 'nextjs-app', 'nextjs-pages', 'vite'])
const VALID_SEVERITIES = new Set<SeverityValue>(['error', 'warn', 'info'])
const KNOWN_TOP_LEVEL_KEYS = new Set([
	'tsconfig',
	'entrypoints',
	'framework',
	'countTestsAsConsumers',
	'allowedExports',
	'outputDir',
	'dryRunByDefault',
	'ignore',
	'detectors',
])
const KNOWN_IGNORE_KEYS = new Set(['files', 'symbols', 'paths'])
const KNOWN_DETECTOR_KEYS = new Set(['enabled', 'severity', 'ignore'])

function resolveFrom(baseDir: string, value: string): string {
	return isAbsolute(value) ? value : resolve(baseDir, value)
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function sourceSuffix(source?: string): string {
	return source ? ` in ${source}` : ''
}

function failConfig(message: string): never {
	throw new Error(`Invalid kuizer config${message}`)
}

function expectString(value: unknown, path: string, source?: string): string {
	if (typeof value !== 'string') {
		failConfig(`: expected ${path} to be a string${sourceSuffix(source)}`)
	}

	return value
}

function expectBoolean(value: unknown, path: string, source?: string): boolean {
	if (typeof value !== 'boolean') {
		failConfig(`: expected ${path} to be a boolean${sourceSuffix(source)}`)
	}

	return value
}

function expectStringArray(value: unknown, path: string, source?: string): string[] {
	if (!Array.isArray(value) || value.some((item) => typeof item !== 'string')) {
		failConfig(`: expected ${path} to be an array of strings${sourceSuffix(source)}`)
	}

	return value
}

function validateIgnore(value: unknown, source?: string): NonNullable<Raw['ignore']> {
	if (!isRecord(value)) {
		failConfig(`: expected ignore to be an object${sourceSuffix(source)}`)
	}

	for (const key of Object.keys(value)) {
		if (!KNOWN_IGNORE_KEYS.has(key)) {
			failConfig(`: unknown ignore key '${key}'${sourceSuffix(source)}`)
		}
	}

	return {
		...(value.files !== undefined
			? { files: expectStringArray(value.files, 'ignore.files', source) }
			: {}),
		...(value.symbols !== undefined
			? { symbols: expectStringArray(value.symbols, 'ignore.symbols', source) }
			: {}),
		...(value.paths !== undefined
			? { paths: expectStringArray(value.paths, 'ignore.paths', source) }
			: {}),
	}
}

function validateDetectorOptions(
	value: unknown,
	path: string,
	source?: string
): Record<string, unknown> {
	if (!isRecord(value)) {
		failConfig(`: expected ${path} to be an object${sourceSuffix(source)}`)
	}

	for (const key of Object.keys(value)) {
		if (!KNOWN_DETECTOR_KEYS.has(key)) {
			failConfig(`: unknown detector option '${path}.${key}'${sourceSuffix(source)}`)
		}
	}

	return {
		...(value.enabled !== undefined
			? { enabled: expectBoolean(value.enabled, `${path}.enabled`, source) }
			: {}),
		...(value.severity !== undefined
			? (() => {
					const severity = expectString(value.severity, `${path}.severity`, source)
					if (!VALID_SEVERITIES.has(severity as SeverityValue)) {
						failConfig(
							`: invalid severity '${severity}' for ${path}.severity${sourceSuffix(source)}`
						)
					}

					return { severity }
				})()
			: {}),
		...(value.ignore !== undefined
			? { ignore: expectStringArray(value.ignore, `${path}.ignore`, source) }
			: {}),
	}
}

function validateRawConfig(config: unknown, source?: string): Raw {
	if (!isRecord(config)) {
		failConfig(`: expected the config file to export an object${sourceSuffix(source)}`)
	}

	for (const key of Object.keys(config)) {
		if (!KNOWN_TOP_LEVEL_KEYS.has(key)) {
			failConfig(`: unknown top-level key '${key}'${sourceSuffix(source)}`)
		}
	}

	const raw: Raw = {}

	if (config.tsconfig !== undefined) {
		raw.tsconfig = expectString(config.tsconfig, 'tsconfig', source)
	}

	if (config.entrypoints !== undefined) {
		raw.entrypoints = expectStringArray(config.entrypoints, 'entrypoints', source)
	}

	if (config.framework !== undefined) {
		const framework = expectString(config.framework, 'framework', source) as Framework
		if (!VALID_FRAMEWORKS.has(framework)) {
			failConfig(`: invalid framework '${framework}'${sourceSuffix(source)}`)
		}

		raw.framework = framework
	}

	if (config.countTestsAsConsumers !== undefined) {
		raw.countTestsAsConsumers = expectBoolean(
			config.countTestsAsConsumers,
			'countTestsAsConsumers',
			source
		)
	}

	if (config.allowedExports !== undefined) {
		raw.allowedExports = expectStringArray(config.allowedExports, 'allowedExports', source)
	}

	if (config.outputDir !== undefined) {
		raw.outputDir = expectString(config.outputDir, 'outputDir', source)
	}

	if (config.dryRunByDefault !== undefined) {
		raw.dryRunByDefault = expectBoolean(config.dryRunByDefault, 'dryRunByDefault', source)
	}

	if (config.ignore !== undefined) {
		raw.ignore = validateIgnore(config.ignore, source)
	}

	if (config.detectors !== undefined) {
		if (!isRecord(config.detectors)) {
			failConfig(`: expected detectors to be an object${sourceSuffix(source)}`)
		}

		raw.detectors = Object.fromEntries(
			Object.entries(config.detectors).map(([detectorId, value]) => [
				detectorId,
				validateDetectorOptions(value, `detectors.${detectorId}`, source),
			])
		) as Config['detectors']
	}

	return raw
}

function isGlobalConfigPath(path: string): boolean {
	return (
		dirname(path) === homedir() &&
		['.kuizerrc', '.kuizerrc.json', '.kuizerrc.js', '.kuizerrc.ts'].includes(basename(path))
	)
}

async function loadGlobalConfig(explorer: ReturnType<typeof cosmiconfig>) {
	const candidates = ['.kuizerrc', '.kuizerrc.json', '.kuizerrc.js', '.kuizerrc.ts'].map((name) =>
		resolve(homedir(), name)
	)

	for (const candidate of candidates) {
		if (!existsSync(candidate)) {
			continue
		}

		return await explorer.load(candidate)
	}

	return undefined
}

/** Merges user-supplied raw config with framework preset and defaults. */
function mergeConfig(raw: Raw, baseDir: string): Config {
	const framework = raw.framework ?? defaults.framework
	const p = preset(framework)

	return {
		tsconfig: resolveFrom(baseDir, raw.tsconfig ?? defaults.tsconfig),
		framework,
		countTestsAsConsumers: raw.countTestsAsConsumers ?? defaults.countTestsAsConsumers,
		outputDir: resolveFrom(baseDir, raw.outputDir ?? defaults.outputDir),
		entrypoints: [...p.entrypoints, ...(raw.entrypoints ?? [])].map((entry) =>
			resolveFrom(baseDir, entry)
		),
		allowedExports: [...p.allowedExports, ...(raw.allowedExports ?? [])],
		ignore: {
			files: [...(p.ignore?.files ?? []), ...(raw.ignore?.files ?? [])],
			symbols: [...(p.ignore?.symbols ?? []), ...(raw.ignore?.symbols ?? [])],
			paths: [...(p.ignore?.paths ?? []), ...(raw.ignore?.paths ?? [])],
		},
		detectors: raw.detectors ?? {},
		dryRunByDefault: raw.dryRunByDefault ?? defaults.dryRunByDefault,
	}
}

/** Discovers and loads kuizer config via cosmiconfig, returning a fully resolved Config. */
export async function loadConfig(
	cwd = process.cwd(),
	configPath?: string,
	tsconfigPath?: string
): Promise<Config> {
	const explorer = cosmiconfig('kuizer', {
		searchPlaces: [
			'kuizer.config.json',
			'kuizer.config.ts',
			'kuizer.config.js',
			'.kuizerrc',
			'.kuizerrc.ts',
			'.kuizerrc.js',
			'.kuizerrc.json',
			'package.json',
		],
	})

	const localResult = configPath
		? await explorer.load(resolve(cwd, configPath))
		: await explorer.search(cwd)
	const result = localResult ?? (configPath ? undefined : await loadGlobalConfig(explorer))

	const loadedPath = result?.filepath
	const baseDir = loadedPath && !isGlobalConfigPath(loadedPath) ? dirname(loadedPath) : cwd
	const raw = validateRawConfig(result?.config ?? {}, loadedPath)
	const configuredTsconfig = raw.tsconfig ? resolveFrom(baseDir, raw.tsconfig) : undefined

	const detectedTsconfig =
		(await detectTsConfig(
			cwd,
			tsconfigPath ? resolve(cwd, tsconfigPath) : configuredTsconfig
		)) ?? resolveFrom(baseDir, defaults.tsconfig)

	const autoDetectedFramework = raw.framework ?? detectFramework(baseDir)

	return mergeConfig({ ...raw, tsconfig: detectedTsconfig, framework: autoDetectedFramework }, baseDir)
}

function detectFramework(baseDir: string): Framework {
	const hasAppRouter = existsSync(resolve(baseDir, 'app')) || existsSync(resolve(baseDir, 'src/app'))
	const hasPagesRouter = existsSync(resolve(baseDir, 'pages')) || existsSync(resolve(baseDir, 'src/pages'))
	const hasViteConfig = existsSync(resolve(baseDir, 'vite.config.ts')) || existsSync(resolve(baseDir, 'vite.config.js'))

	if (hasAppRouter) return 'nextjs-app'
	if (hasPagesRouter) return 'nextjs-pages'
	if (hasViteConfig) return 'vite'
	return 'none'
}
