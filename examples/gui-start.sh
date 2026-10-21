#!/usr/bin/env bash

set -Eeuo pipefail

PROJECT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
GUI="${KUIZER_GUI_BIN:-$PROJECT_ROOT/target/release/kuizer-gui}"
RUN_SECONDS="${RUN_SECONDS:-4}"
TRACE="$(mktemp "${TMPDIR:-/tmp}/kuizer-gui-start.XXXXXX")"

trap 'rm -f -- "$TRACE"' EXIT

function require() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'gui-start: %s is required\n' "$1" >&2
        exit 1
    fi
}

function first_time() {
    PATTERN="$1" awk '$0 ~ ENVIRON["PATTERN"] { print $2; exit }' "$TRACE"
}

function millis() {
    awk -v from="$1" -v to="$2" 'BEGIN { printf "%.1f", (to - from) * 1000 }'
}

function report() {
    local label="$1" stamp="$2" exec_at="$3"
    if [[ -z "$stamp" ]]; then
        printf '%-28s not seen\n' "$label"
    else
        printf '%-28s %8s ms\n' "$label" "$(millis "$exec_at" "$stamp")"
    fi
}

require strace
[[ -x "$GUI" ]] || { printf 'Build first: cargo build --release -p kuizer-gui\n' >&2; exit 1; }

export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}"

strace -f -ttt -e trace=openat,execve -o "$TRACE" -- timeout "$RUN_SECONDS" "$GUI" >/dev/null 2>&1 || true

exec_at="$(first_time "execve\\(\"$GUI\"")"
if [[ -z "$exec_at" ]]; then
    printf 'gui-start: the binary never started\n' >&2
    exit 1
fi

gpu_at="$(first_time 'openat\(.*"/dev/(nvidia|dri/renderD)[^"]*".*= [0-9]')"
recent_at="$(first_time 'openat\(.*gui-recent".*= [0-9]')"

report 'first GPU device open' "$gpu_at" "$exec_at"
report 'first gui-recent read' "$recent_at" "$exec_at"
if [[ -n "$gpu_at" && -n "$recent_at" ]]; then
    printf '%-28s %8s ms\n' 'window build after GPU open' "$(millis "$gpu_at" "$recent_at")"
fi
