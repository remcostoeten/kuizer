import type { Framework, Config } from '../types.js'

type Preset = Pick<Config, 'entrypoints' | 'allowedExports'>

const presets = {
	'nextjs-app': {
		entrypoints: [
			'app/**/page.tsx',
			'app/**/layout.tsx',
			'app/**/loading.tsx',
			'app/**/error.tsx',
			'app/**/not-found.tsx',
			'app/**/route.ts',
			'app/**/template.tsx',
			'app/**/default.tsx',
			'middleware.ts',
			'instrumentation.ts',
		],
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
	},
	'nextjs-pages': {
		entrypoints: [
			'pages/**/*.tsx',
			'pages/**/*.ts',
			'pages/_app.tsx',
			'pages/_document.tsx',
			'pages/_error.tsx',
			'pages/api/**/*.ts',
		],
		allowedExports: ['getServerSideProps', 'getStaticProps', 'getStaticPaths'],
	},
	vite: {
		entrypoints: ['src/main.tsx', 'src/main.ts', 'index.html'],
		allowedExports: [] as string[],
	},
	none: {
		entrypoints: [] as string[],
		allowedExports: [] as string[],
	},
} as const satisfies Record<Framework, Preset>

/** Returns the immutable entry point and export allowlist for a given framework. */
export function preset(framework: Framework): Preset {
	return presets[framework]
}
