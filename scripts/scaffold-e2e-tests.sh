#!/usr/bin/env bash
set -euo pipefail

MAX_BACKUPS=5
BACKUP_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/kuizer/backups"

list_backups() {
  if [ ! -d "$BACKUP_DIR" ] || [ -z "$(ls -A "$BACKUP_DIR")" ]; then
    echo "No backups found in $BACKUP_DIR"
    exit 0
  fi
  echo "Available backups:"
  ls -1t "$BACKUP_DIR"
  exit 0
}

diff_backup() {
  local backup_name="$1"
  local target_backup="$BACKUP_DIR/$backup_name"
  if [ ! -d "$target_backup" ]; then
    echo "Backup $backup_name not found in $BACKUP_DIR"
    exit 1
  fi
  local target_root="${2:-tests/fixtures/sample}"
  
  echo "Diff between $target_backup and $target_root:"
  for new_file in $(find "$target_root" -type f -name "*.ts" -o -name "*.json" | sort); do
    rel="${new_file#$target_root/}"
    old_file="$target_backup/$rel"
    if [ -f "$old_file" ]; then
      if ! diff -q "$old_file" "$new_file" > /dev/null 2>&1; then
        echo "--- $rel (old)"
        echo "+++ $rel (new)"
        diff -u "$old_file" "$new_file" | tail -n +3 || true
      fi
    fi
  done
  exit 0
}

if [ "${1:-}" == "--list-backups" ]; then
  list_backups
fi

if [ "${1:-}" == "--diff-backup" ]; then
  if [ -z "${2:-}" ]; then
    echo "Usage: $0 --diff-backup <backup-name> [target-root]"
    exit 1
  fi
  diff_backup "$2" "${3:-tests/fixtures/sample}"
fi

ROOT="${1:-tests/fixtures/sample}"

mkdir -p \
  "$ROOT/src/components" \
  "$ROOT/src/utils" \
  "$ROOT/src/lib" \
  "$ROOT/src/types"

if [ -f "$ROOT/src/index.ts" ]; then
  read -p "Fixture already exists at $ROOT. Overwrite? [y/N] " confirm
  if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 0
  fi
  BACKUP_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/kuizer/backups"
  mkdir -p "$BACKUP_DIR"
  BACKUP_NAME="$(basename "$ROOT").backup.$(date +%Y%m%d_%H%M%S)"
  BACKUP="$BACKUP_DIR/$BACKUP_NAME"
  cp -r "$ROOT" "$BACKUP"
  echo "Backup created at $BACKUP"
  rm -rf "$ROOT"

  base_name="$(basename "$ROOT")"
  backup_count=$(ls -1td "$BACKUP_DIR"/${base_name}.backup.* 2>/dev/null | wc -l || echo 0)
  if [ "$backup_count" -gt "$MAX_BACKUPS" ]; then
    diff_count=$((backup_count - MAX_BACKUPS))
    ls -1td "$BACKUP_DIR"/${base_name}.backup.* | tail -n "$diff_count" | xargs rm -rf
    echo "Pruned $diff_count old backup(s)."
  fi
  mkdir -p \
    "$ROOT/src/components" \
    "$ROOT/src/utils" \
    "$ROOT/src/lib" \
    "$ROOT/src/types"
  OLD_BACKUP="$BACKUP"
else
  OLD_BACKUP=""
fi

# ── entry point ──────────────────────────────────────────────────────────────

  # ── color codes ──
  BOLD="\033[1m"
  CYAN="\033[36m"
  YELLOW="\033[33m"
  GREEN="\033[32m"
  MAGENTA="\033[35m"
  GRAY="\033[90m"
  RESET="\033[0m"


  echo -e "${BOLD}Expected findings:${RESET}"
  echo -e "  ${YELLOW}unusedFiles${RESET}    ${GRAY}→${RESET} ${GREEN}src/utils/format.ts${RESET}"
  echo -e "                 ${GRAY}→${RESET} ${GREEN}src/utils/math.ts${RESET}"
  echo -e "                 ${GRAY}→${RESET} ${GREEN}src/components/Button.ts${RESET}"
  echo -e "                 ${GRAY}→${RESET} ${GREEN}src/components/Modal.ts${RESET}"
  echo -e "  ${YELLOW}unusedExports${RESET}  ${GRAY}→${RESET} ${GREEN}src/lib/greet.ts${RESET}        ${MAGENTA}: shout${RESET}"
  echo -e "                 ${GRAY}→${RESET} ${GREEN}src/lib/logger.ts${RESET}       ${MAGENTA}: FileLogger${RESET}"
  echo -e "                 ${GRAY}→${RESET} ${GREEN}src/utils/format.ts${RESET}     ${MAGENTA}: formatDate, formatBytes, formatPercent${RESET}"
  echo -e "                 ${GRAY}→${RESET} ${GREEN}src/types/index.ts${RESET}      ${MAGENTA}: Timestamp, PaginatedResponse${RESET}"
cat >"$ROOT/src/index.ts" <<'EOF'
import { greet } from './lib/greet.js'
import { Logger } from './lib/logger.js'

const logger = new Logger('app')
logger.info(greet('world'))
EOF

# ── live: lib/greet.ts ────────────────────────────────────────────────────────

cat >"$ROOT/src/lib/greet.ts" <<'EOF'
export function greet(name: string): string {
  return `Hello, ${name}`
}

export function shout(name: string): string {
  return `HELLO, ${name.toUpperCase()}`
}
EOF

# ── live: lib/logger.ts ───────────────────────────────────────────────────────

cat >"$ROOT/src/lib/logger.ts" <<'EOF'
export class Logger {
  constructor(private readonly name: string) {}

  info(msg: string) {
    console.log(`[${this.name}] ${msg}`)
  }
}

export class FileLogger {
  constructor(private readonly path: string) {}

  write(msg: string) {
    console.log(`[file:${this.path}] ${msg}`)
  }
}
EOF

# ── dead: utils/format.ts — exported, never imported anywhere ─────────────────

cat >"$ROOT/src/utils/format.ts" <<'EOF'
export function formatDate(d: Date): string {
  return d.toISOString().split('T')[0] ?? ''
}

export function formatBytes(n: number): string {
  return `${(n / 1024).toFixed(2)} KB`
}

export function formatPercent(n: number): string {
  return `${(n * 100).toFixed(1)}%`
}
EOF

# ── dead: utils/math.ts — entire file unreachable ────────────────────────────

cat >"$ROOT/src/utils/math.ts" <<'EOF'
export function clamp(n: number, min: number, max: number): number {
  return Math.min(Math.max(n, min), max)
}

export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t
}
EOF

# ── dead: components/Button.ts — unreachable, no consumers ───────────────────

cat >"$ROOT/src/components/Button.ts" <<'EOF'
export type ButtonVariant = 'primary' | 'secondary' | 'ghost'

export type ButtonProps = {
  label:    string
  variant?: ButtonVariant
  disabled?: boolean
}

export function renderButton(props: ButtonProps): string {
  return `<button class="${props.variant ?? 'primary'}">${props.label}</button>`
}
EOF

# ── dead: components/Modal.ts — unreachable ───────────────────────────────────

cat >"$ROOT/src/components/Modal.ts" <<'EOF'
export type ModalProps = {
  title:   string
  content: string
}

export function renderModal(props: ModalProps): string {
  return `<dialog><h2>${props.title}</h2><p>${props.content}</p></dialog>`
}
EOF

# ── mixed: types/index.ts — reachable (imported below) but has dead exports ──

cat >"$ROOT/src/types/index.ts" <<'EOF'
export type ID = string

export type Timestamp = number

export type Result<T> =
  | { ok: true;  value: T }
  | { ok: false; error: string }

export type PaginatedResponse<T> = {
  items:   T[]
  total:   number
  page:    number
  perPage: number
}
EOF

# make index.ts consume ID and Result so those stay live
cat >>"$ROOT/src/index.ts" <<'EOF'

import type { ID, Result } from './types/index.js'

function resolveId(id: ID): Result<string> {
  return { ok: true, value: id }
}

resolveId('bootstrap')
EOF


# ── README.md─────────────────────────────────────────────────────────────────
date=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
cat >"$ROOT/README.md" <<'EOF'
# Sample Project

> Scaffolded via `scaffold-e2e-tests.sh` on '"$date"',

This project is a sample codebase used for testing the unused code detectors in Kuizer. It contains a mix of live and dead code, including:

- **Live code**: Code that is reachable and used by the entry point (`src/index.ts`).
- **Dead code**: Code that is unreachable or unused, such as utility functions that are never imported, or entire files that have no consumers.

Dead code:
- `src/utils/format.ts`: Exported utility functions that are never imported anywhere.
- `src/utils/math.ts`: Contains utility functions that are never imported anywhere.
- `src/components/Button.ts`: A UI component that is defined but never used.
- `src/components/Modal.ts`: Another UI component that is defined but never used.
EOF
# ── tsconfig ─────────────────────────────────────────────────────────────────

cat >"$ROOT/tsconfig.json" <<'EOF'
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noEmit": true
  },
  "include": ["src"]
}
EOF

# ── kuizer config ─────────────────────────────────────────────────────────────

cat >"$ROOT/kuizer.config.json" <<'EOF'
{
  "kuizer": {
    "tsconfig": "./tsconfig.json",
    "entrypoints": ["src/index.ts"],
    "framework": "none"
  }
}
EOF

echo ""
echo "Fixture scaffolded at $ROOT"
echo ""
echo "Expected findings:"
echo "  unusedFiles    → src/utils/format.ts"
echo "                 → src/utils/math.ts"
echo "                 → src/components/Button.ts"
echo "                 → src/components/Modal.ts"
echo "  unusedExports  → src/lib/greet.ts        : shout"
echo "                 → src/lib/logger.ts       : FileLogger"
echo "                 → src/utils/format.ts     : formatDate, formatBytes, formatPercent"
echo "                 → src/types/index.ts      : Timestamp, PaginatedResponse"

if [ -n "$OLD_BACKUP" ]; then
  echo ""
  echo "Diff:"
  for new_file in $(find "$ROOT" -type f -name "*.ts" -o -name "*.json" | sort); do
    rel="${new_file#$ROOT/}"
    old_file="$OLD_BACKUP/$rel"
    if [ -f "$old_file" ]; then
      if ! diff -q "$old_file" "$new_file" > /dev/null 2>&1; then
        echo "--- $rel (old)"
        echo "+++ $rel (new)"
        diff -u "$old_file" "$new_file" | tail -n +3 || true
      fi
    fi
  done
fi
echo ""
