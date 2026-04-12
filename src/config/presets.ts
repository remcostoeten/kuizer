import type { Framework, Config } from '../types.js'

type Preset = Pick<Config, 'entrypoints' | 'allowedExports' | 'ignore'>

const NEXT_APP_ENTRYPOINTS = [
	'src/app/**/page.tsx',
	'src/app/**/layout.tsx',
	'src/app/**/loading.tsx',
	'src/app/**/error.tsx',
	'src/app/**/global-error.tsx',
	'src/app/**/not-found.tsx',
	'src/app/**/route.ts',
	'src/app/**/route.tsx',
	'src/app/**/template.tsx',
	'src/app/**/default.tsx',
	'src/app/**/manifest.ts',
	'src/app/**/robots.ts',
	'src/app/**/sitemap.ts',
	'src/app/**/opengraph-image.tsx',
	'src/app/**/twitter-image.tsx',
	'src/app/**/icon.tsx',
	'src/app/**/apple-icon.tsx',
	'src/middleware.ts',
	'src/proxy.ts',
	'src/instrumentation.ts',
	'app/**/page.tsx',
	'app/**/layout.tsx',
	'app/**/loading.tsx',
	'app/**/error.tsx',
	'app/**/global-error.tsx',
	'app/**/not-found.tsx',
	'app/**/route.ts',
	'app/**/route.tsx',
	'app/**/template.tsx',
	'app/**/default.tsx',
	'app/**/manifest.ts',
	'app/**/robots.ts',
	'app/**/sitemap.ts',
	'app/**/opengraph-image.tsx',
	'app/**/twitter-image.tsx',
	'app/**/icon.tsx',
	'app/**/apple-icon.tsx',
	'middleware.ts',
	'proxy.ts',
	'instrumentation.ts',
] as const

const NEXT_DEFAULT_IGNORE_FILES = [
	'next-env.d.ts',
	'next.config.ts',
	'next.config.js',
	'next.config.mjs',
	'drizzle.config.ts',
	'drizzle.config.js',
	'drizzle.config.mjs',
	'vitest.config.ts',
	'vitest.config.js',
	'vitest.config.mjs',
	'**/test/setup.ts',
	'**/test/setup.tsx',
	'**/tests/setup.ts',
	'**/tests/setup.tsx',
] as const

const presets = {
	'nextjs-app': {
		entrypoints: [...NEXT_APP_ENTRYPOINTS],
		allowedExports: [
			'generateMetadata',
			'generateStaticParams',
			'generateViewport',
			'revalidate',
			'dynamic',
			'runtime',
			'fetchCache',
			'preferredRegion',
			'maxDuration',
		],
		ignore: {
			paths: ['**/ui/**', '**/components/ui/**'],
			files: [...NEXT_DEFAULT_IGNORE_FILES],
			symbols: [],
		},
	},
	'nextjs-pages': {
		entrypoints: [
			'src/pages/**/*.tsx',
			'src/pages/**/*.ts',
			'src/pages/_app.tsx',
			'src/pages/_document.tsx',
			'src/pages/_error.tsx',
			'src/pages/api/**/*.ts',
			'pages/**/*.tsx',
			'pages/**/*.ts',
			'pages/_app.tsx',
			'pages/_document.tsx',
			'pages/_error.tsx',
			'pages/api/**/*.ts',
		],
		allowedExports: ['getServerSideProps', 'getStaticProps', 'getStaticPaths'],
		ignore: {
			paths: ['**/ui/**', '**/components/ui/**'],
			files: [...NEXT_DEFAULT_IGNORE_FILES],
			symbols: [],
		},
	},
	vite: {
		entrypoints: [
			'src/main.tsx',
			'src/main.ts',
			'src/index.tsx',
			'src/index.ts',
			'index.html',
			'src/App.tsx',
			'src/App.ts',
		],
		allowedExports: [] as string[],
		ignore: {
			paths: [],
			files: [],
			symbols: [],
		},
	},
	none: {
		entrypoints: [] as string[],
		allowedExports: [] as string[],
		ignore: {
			paths: [],
			files: [],
			symbols: [],
		},
	},
} as const satisfies Record<Framework, Preset>

/** Returns the immutable entry point and export allowlist for a given framework. */
export function preset(framework: Framework): Preset {
	return presets[framework]
}
