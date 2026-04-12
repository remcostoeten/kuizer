# Kuizer

Find and remove dead code in TypeScript projects.

## Install

Today, Kuizer is installed from source in this repository. Once you publish the
package to a registry, users can install it globally with Bun or npm, or run it
with `bunx` or `npx`.

### Build from source

```bash
bun install --frozen-lockfile
bun run build
```

### Link globally from this checkout

```bash
# Make `kuizer` available on your PATH from this repository
bun link

# Run it from anywhere on your machine
kuizer detect

# Or run it without linking
bunx /path/to/kuizer detect
```

### After publishing

After Kuizer is published to a package registry, users can install or run it
without cloning the repository.

```bash
# Global install with Bun
bun add -g kuizer

# Global install with npm
npm install -g kuizer

# One-off execution
bunx kuizer detect
npx kuizer detect
```

## Usage

Kuizer supports direct CLI usage, an interactive menu, and generated shell
completion scripts for Bash, Zsh, and Fish.

```bash
# Find dead code
kuizer detect

# Find specific detectors
kuizer detect --only unusedExports
kuizer detect --only unusedFiles,unusedImports

# Filter by export type
kuizer detect -f functions,types,classes

# Preview fixes
kuizer fix --dry

# Apply fixes (creates backup first)
kuizer fix -w

# Restore from backup
kuizer restore --snapshot .kuizer/snapshots/...
kuizer revert --snapshot 2026-04-05T04-45-49-701Z

# Show shell completion setup instructions
kuizer completion

# Print a shell-specific completion script
kuizer completion --shell zsh
```

## Shell completions

Kuizer can print shell completion setup instructions or a shell-specific
completion script. Use the `completion` command without flags to see the
available installation patterns.

```bash
kuizer completion
```

To generate a completion script directly, pass `--shell` and redirect the
output into your shell's completion directory.

```bash
# Bash
kuizer completion --shell bash > ~/.local/share/bash-completion/completions/kuizer

# Zsh
kuizer completion --shell zsh > ~/.zfunc/_kuizer

# Fish
kuizer completion --shell fish > ~/.config/fish/completions/kuizer.fish
```

For Zsh, add `fpath=(~/.zfunc $fpath)` to your `~/.zshrc` if needed, then run
`autoload -Uz compinit && compinit`.

## Example Output

```
$ kuizer detect

src/components/Modal.ts
  warn   L1:C1  unusedFiles  File is unreachable from any entry point

src/utils/format.ts
  warn   L5:C1  unusedFunctions  Exported symbol 'formatPhone' has 0 consumers

src/types/index.ts
  warn   L3:C1  unusedTypes  Exported symbol 'UnusedType' has 0 consumers

────────────────────────────────────────────────────────────
0 errors · 3 warnings · 0.6s
```

## Commands

| Command | Description |
|---------|-------------|
| `detect` | Analyze codebase for dead code |
| `fix` | Remove unused code |
| `restore` | Restore files from snapshot |
| `init` | Create config file |
| `completion` | Print shell completion setup info or scripts |
| `revert` | Revert from snapshot |

## Flags

| Flag | Description |
|------|-------------|
| `-c, --config` | Config file path |
| `-o, --only` | Run specific detectors |
| `-s, --skip` | Skip detectors |
| `-f, --filter` | Filter by export type |
| `-r, --reporter` | Output format (terminal/json) |
| `--json` | JSON output |
| `--interactive` | Interactive TUI |
| `--include-ui` | Include preset UI directories in analysis |
| `--dry-run` | Preview without writing |
| `-w, --write` | Apply fixes |
| `--no-backup` | Skip backup |

## Config

Kuizer looks for config in:
1. `--config <path>`
2. `kuizer.config.json` in current directory
3. `~/.kuizerrc`

## Development

```bash
bun run typecheck
bun run lint
bun run test
bun run build
```
