#!/usr/bin/env bash

set -Eeuo pipefail

PROJECT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/kuizer-demo.XXXXXX")"

if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
    RESET=$'\033[0m'
    BOLD=$'\033[1m'
    DIM=$'\033[2m'
    CYAN=$'\033[36m'
    YELLOW=$'\033[33m'
else
    RESET=''
    BOLD=''
    DIM=''
    CYAN=''
    YELLOW=''
fi

trap 'rm -rf -- "$FIXTURE_ROOT"' EXIT

function resolve_kuizer() {
    if [[ -n "${KUIZER_BIN:-}" ]]; then
        printf '%s' "$KUIZER_BIN"
    elif [[ -x "$PROJECT_ROOT/target/release/kuizer" ]]; then
        printf '%s' "$PROJECT_ROOT/target/release/kuizer"
    elif command -v kuizer >/dev/null 2>&1; then
        command -v kuizer
    else
        printf 'Build kuizer first: cargo build --release\n' >&2
        exit 1
    fi
}

KUIZER="$(resolve_kuizer)"

function write_file() {
    mkdir -p -- "$(dirname -- "$1")"
    head -c "$2" /dev/zero >"$1"
}

function create_fixture() {
    local web="$FIXTURE_ROOT/web"
    local api="$FIXTURE_ROOT/api"
    local scripts="$FIXTURE_ROOT/scripts"

    write_file "$web/package.json" 200
    write_file "$web/node_modules/react/index.js" 400000
    write_file "$web/node_modules/vite/dist/node.js" 900000
    write_file "$web/.next/cache/webpack.pack" 1200000
    write_file "$web/dist/app.js" 300000

    write_file "$api/Cargo.toml" 200
    write_file "$api/target/.rustc_info.json" 200
    printf 'Signature: 8a477f597d28d172789f06886806bc55\n' >"$api/target/CACHEDIR.TAG"
    write_file "$api/target/debug/api" 2500000

    write_file "$scripts/tool.py" 500
    write_file "$scripts/__pycache__/tool.cpython-313.pyc" 20000
}

function print_step() {
    printf '\n%s%s%s %s\n' "$BOLD$YELLOW" "$1" "$RESET" "$2"
    printf '%s%s%s\n' "$DIM" "$3" "$RESET"
}

function print_command() {
    printf '\n%s$ %s%s\n\n' "$CYAN" "$1" "$RESET"
}

function pause() {
    if [[ -t 0 ]]; then
        printf '\n%sPress Enter to continue%s' "$DIM" "$RESET"
        read -r
    fi
}

function run_interactive() {
    if [[ -t 0 && -t 1 ]]; then
        pause
        "$@"
    else
        printf '%sSkipped, needs an interactive terminal.%s\n' "$DIM" "$RESET"
    fi
}

function main() {
    create_fixture
    cd -- "$FIXTURE_ROOT"

    printf '%s◆ kuizer vs kuizer ui%s  %s%s%s\n' "$BOLD$CYAN" "$RESET" "$DIM" "$FIXTURE_ROOT" "$RESET"

    print_step 1 'kuizer in a terminal' 'Opens the UI on the current directory. Quit with q.'
    print_command 'kuizer'
    run_interactive "$KUIZER"

    print_step 2 'kuizer ui in a terminal' 'Same UI, same directory. There is no difference here.'
    print_command 'kuizer ui'
    run_interactive "$KUIZER" ui

    print_step 3 'kuizer ui with paths' 'Only kuizer ui accepts directories. This opens the UI on web/ alone.'
    print_command 'kuizer ui web'
    run_interactive "$KUIZER" ui web

    print_step 4 'kuizer with paths' 'Bare kuizer takes no paths, so this fails.'
    print_command 'kuizer web'
    "$KUIZER" web || true
    pause

    print_step 5 'kuizer piped' 'Without a terminal on stdout, bare kuizer runs a scan instead of the UI.'
    print_command 'kuizer | cat'
    "$KUIZER" | cat
    pause

    print_step 6 'kuizer --json' 'JSON output also skips the UI and prints the scan result.'
    print_command 'kuizer --json | head -c 600'
    "$KUIZER" --json | head -c 600
    printf '\n'
}

main "$@"
