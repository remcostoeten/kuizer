# Kuizer configuration

Kuizer loads project settings from config files and merges them with defaults.

## Lookup order

Kuizer resolves configuration in this order:

1. The file passed with `--config`
2. Project config discovered from the current working directory upward
3. `~/.kuizerrc`
4. Built-in defaults

Project discovery checks these files:

- `kuizer.config.json`
- `kuizer.config.ts`
- `kuizer.config.js`
- `.kuizerrc`
- `.kuizerrc.json`
- `.kuizerrc.ts`
- `.kuizerrc.js`
- `package.json` under the `kuizer` key

## Supported keys

These are the currently supported top-level config keys:

```json
{
	"tsconfig": "./tsconfig.json",
	"entrypoints": ["src/index.ts"],
	"framework": "none",
	"countTestsAsConsumers": false,
	"allowedExports": ["generateMetadata"],
	"outputDir": ".kuizer",
	"dryRunByDefault": true,
	"ignore": {
		"files": ["src/generated/example.ts"],
		"symbols": ["^_"],
		"paths": ["src/legacy"]
	},
	"detectors": {
		"unusedExports": {
			"enabled": true,
			"severity": "warn"
		},
		"unusedFiles": {
			"enabled": true,
			"severity": "warn"
		},
		"unusedImports": {
			"enabled": false,
			"severity": "warn"
		},
		"unusedFunctions": {
			"enabled": true,
			"severity": "warn"
		}
	}
}
```

## TypeScript config resolution

Kuizer resolves the TypeScript project in this order:

1. `--tsconfig <path>`
2. `tsconfig` from config files
3. The nearest `tsconfig.json` from the current working directory upward
4. The default `./tsconfig.json` relative to the selected config base

## `dryRunByDefault`

Set `dryRunByDefault` to control the default behavior of `kuizer fix`.

When it is `true`:

```bash
kuizer fix
```

prints a dry-run plan.

When it is `false`:

```bash
kuizer fix
```

applies supported fixes immediately.

Command-line flags still win:

```bash
kuizer fix --dry
kuizer fix -w
```

## Framework presets

The current framework presets are:

- `none`
- `nextjs-app`
- `nextjs-pages`
- `vite`

Presets contribute entrypoints and allowed exports.
