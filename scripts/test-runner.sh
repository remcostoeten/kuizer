#!/usr/bin/env bash
set -euo pipefail

# ── terminal capabilities ────────────────────────────────────────────────────

BOLD='\033[1m'
DIM='\033[2m'
RESET='\033[0m'
GREEN='\033[38;5;114m'
RED='\033[38;5;203m'
YELLOW='\033[38;5;221m'
CYAN='\033[38;5;117m'
WHITE='\033[38;5;255m'
GRAY='\033[38;5;242m'
MAGENTA='\033[38;5;176m'
BG_DARK='\033[48;5;234m'
BG_GREEN='\033[48;5;22m'
BG_RED='\033[48;5;52m'

WIDTH=64

# ── primitives ───────────────────────────────────────────────────────────────

rule() {
  local char="${1:-─}"
  local i
  for (( i = 0; i < WIDTH; i++ )); do
    printf '%s' "$char"
  done
  printf '\n'
}

colored_rule() {
  local char="${1:-─}"
  local color="${2:-$GRAY}"
  printf '%b' "$color"
  rule "$char"
  printf '%b' "$RESET"
}

badge() {
  local label="$1"
  local fg="$2"
  local bg="${3:-$BG_DARK}"
  printf '%b' "${bg}${fg}${BOLD} ${label} ${RESET}"
}

section_start() {
  echo ""
  printf '%b' "${GRAY}  ╭─ ${RESET}${BOLD}${WHITE}${1}${RESET}"
  printf '\n'
}

section_row() {
  printf '%b' "${GRAY}  │  ${RESET}"
  printf '%b' "$1"
  printf '\n'
}

section_blank() {
  printf '%b' "${GRAY}  │${RESET}\n"
}

section_end() {
  printf '%b' "${GRAY}  ╰─${RESET}\n"
}

# ── header ───────────────────────────────────────────────────────────────────

print_header() {
  if [ "${CI:-}" != "true" ] && [ -t 1 ] && [ -n "${TERM:-}" ]; then
    clear || true
  fi
  echo ""
  colored_rule "═" "$CYAN"
  printf "  ${BOLD}${WHITE}KUIZER${RESET}  ${GRAY}test suite${RESET}"
  local time_str
  time_str=$(date '+%H:%M:%S')
  printf "%*s" $(( WIDTH - 24 )) ""
  printf "${DIM}%s${RESET}\n" "$time_str"
  colored_rule "═" "$CYAN"
}

# ── environment info ─────────────────────────────────────────────────────────

print_env() {
  section_start "Environment"

  local node_ver bun_ver ts_ver git_branch git_sha
  bun_ver=$(bun --version 2>/dev/null || echo "n/a")
  ts_ver=$(./node_modules/.bin/tsc --version 2>/dev/null | awk '{print $2}' || echo "n/a")
  node_ver=$(node --version 2>/dev/null || echo "n/a")
  git_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "n/a")
  git_sha=$(git rev-parse --short HEAD 2>/dev/null || echo "n/a")

  section_row "${GRAY}runtime${RESET}      ${WHITE}bun ${bun_ver}${RESET}"
  section_row "${GRAY}typescript${RESET}   ${WHITE}${ts_ver}${RESET}"
  section_row "${GRAY}node${RESET}         ${WHITE}${node_ver}${RESET}"
  section_row "${GRAY}branch${RESET}       ${CYAN}${git_branch}${RESET}  ${GRAY}${git_sha}${RESET}"
  section_row "${GRAY}ci${RESET}           ${WHITE}${CI:-false}${RESET}"

  section_end
}

# ── typecheck ────────────────────────────────────────────────────────────────

run_typecheck() {
  section_start "Typecheck"

  local start end elapsed output exit_code
  start=$(date +%s%3N)

  set +e
  output=$(./node_modules/.bin/tsc --noEmit 2>&1)
  exit_code=$?
  set -e

  end=$(date +%s%3N)
  elapsed=$(( end - start ))

  if [ $exit_code -eq 0 ]; then
    section_row "$(badge "PASS" "$GREEN" "$BG_GREEN")  ${GRAY}tsc --noEmit${RESET}  ${GRAY}${elapsed}ms${RESET}"
  else
    section_row "$(badge "FAIL" "$RED" "$BG_RED")  ${GRAY}tsc --noEmit${RESET}  ${GRAY}${elapsed}ms${RESET}"
    section_blank
    # Show errors indented, max 10 lines
    local count=0
    while IFS= read -r line; do
      if [ -n "$line" ]; then
        section_row "  ${RED}${line}${RESET}"
        (( count++ )) || true
        if [ $count -ge 10 ]; then
          section_row "  ${DIM}... (truncated)${RESET}"
          break
        fi
      fi
    done <<< "$output"
  fi

  section_end
  return $exit_code
}

# ── run tests + parse ────────────────────────────────────────────────────────

# Temp files for inter-function communication (avoids subshell issues)
RESULTS_FILE=$(mktemp)
FAILURES_FILE=$(mktemp)
trap 'rm -f "$RESULTS_FILE" "$FAILURES_FILE"' EXIT

run_tests() {
  local start end elapsed raw exit_code
  start=$(date +%s%3N)

  set +e
  raw=$(bun test 2>&1)
  exit_code=$?
  set -e

  end=$(date +%s%3N)
  elapsed=$(( end - start ))

  # ── parse + display ──
  section_start "Tests"

  local pass=0
  local fail=0
  local current_file=""

  # Strip ANSI codes for parsing
  local stripped
  stripped=$(echo "$raw" | sed 's/\x1b\[[0-9;]*m//g')

  while IFS= read -r line; do
    # Skip empty lines
    [[ -z "$line" ]] && continue

    # File header — "path/to/file.test.ts:"
    if [[ "$line" =~ ^([^ ].+\.test\.ts):$ ]]; then
      current_file="${BASH_REMATCH[1]}"
      local short
      short=$(echo "$current_file" | sed 's|^src/||' | sed 's|^__tests__/||')
      section_blank
      section_row "${MAGENTA}${short}${RESET}"
      continue
    fi

    # Passing test: "✓ test name [0.55ms]" or "(pass) test name [0.55ms]"
    if [[ "$line" =~ ^[[:space:]]*(✓|\(pass\))[[:space:]]+(.+) ]]; then
      local name="${BASH_REMATCH[2]}"
      local ms=""
      if [[ "$name" =~ ^(.+)[[:space:]]+\[([0-9.]+)ms\]$ ]]; then
        name="${BASH_REMATCH[1]}"
        ms="${BASH_REMATCH[2]}"
      fi
      # Strip describe group prefix
      name=$(echo "$name" | sed 's/^[^>]*> //')
      if [ -n "$ms" ]; then
        section_row "  ${GREEN}✓${RESET}  ${GRAY}${name}${RESET}  ${DIM}${ms}ms${RESET}"
      else
        section_row "  ${GREEN}✓${RESET}  ${GRAY}${name}${RESET}"
      fi
      (( pass++ )) || true
      continue
    fi

    # Failing test: "✗ test name" or "⨯ test name" or "(fail) test name"
    if [[ "$line" =~ ^[[:space:]]*(✗|⨯|✖|\(fail\))[[:space:]]+(.+) ]]; then
      local name="${BASH_REMATCH[2]}"
      if [[ "$name" =~ ^(.+)[[:space:]]+\[([0-9.]+)ms\]$ ]]; then
        name="${BASH_REMATCH[1]}"
      fi
      name=$(echo "$name" | sed 's/^[^>]*> //')
      section_row "  ${RED}✗${RESET}  ${WHITE}${name}${RESET}"
      echo "${current_file}: ${name}" >> "$FAILURES_FILE"
      (( fail++ )) || true
      continue
    fi

    # Bun summary line — "X pass" / "Y fail"
    if [[ "$line" =~ ^[[:space:]]*([0-9]+)[[:space:]]pass ]]; then
      pass="${BASH_REMATCH[1]}"
    fi
    if [[ "$line" =~ ^[[:space:]]*([0-9]+)[[:space:]]fail ]]; then
      fail="${BASH_REMATCH[1]}"
    fi

  done <<< "$stripped"

  section_end

  # Write pass/fail counts for main to read
  echo "$pass $fail $elapsed" > "$RESULTS_FILE"

  return $exit_code
}

# ── summary ──────────────────────────────────────────────────────────────────

print_summary() {
  local pass="$1"
  local fail="$2"
  local elapsed_ms="$3"

  # Guard against empty values
  pass="${pass:-0}"
  fail="${fail:-0}"
  elapsed_ms="${elapsed_ms:-0}"

  local total=$(( pass + fail ))
  local elapsed_s
  elapsed_s=$(awk "BEGIN { printf \"%.1f\", ${elapsed_ms} / 1000 }")

  echo ""
  colored_rule "─" "$GRAY"

  if [ "$fail" -eq 0 ] && [ "$total" -gt 0 ]; then
    printf "  $(badge "ALL PASS" "$GREEN" "$BG_GREEN")  "
    printf "${WHITE}${BOLD}%d${RESET} ${GRAY}tests${RESET}  " "$total"
    printf "${GRAY}%ss${RESET}\n" "$elapsed_s"
  elif [ "$total" -eq 0 ]; then
    printf "  $(badge "NO TESTS" "$YELLOW")  "
    printf "${GRAY}no test results captured${RESET}\n"
  else
    printf "  $(badge "FAILED" "$RED" "$BG_RED")  "
    printf "${GREEN}%d passed${RESET}  " "$pass"
    printf "${RED}%d failed${RESET}  " "$fail"
    printf "${GRAY}%d total  %ss${RESET}\n" "$total" "$elapsed_s"

    echo ""
    printf "  ${GRAY}Failures:${RESET}\n"
    if [ -s "$FAILURES_FILE" ]; then
      while IFS= read -r f; do
        printf "  ${RED}↳${RESET}  ${WHITE}%s${RESET}\n" "$f"
      done < "$FAILURES_FILE"
    fi
  fi

  colored_rule "─" "$GRAY"
  echo ""
}

# ── main ─────────────────────────────────────────────────────────────────────

main() {
  print_header
  print_env

  local typecheck_ok=true
  run_typecheck || typecheck_ok=false

  local test_ok=true
  run_tests || test_ok=false

  # Read results
  local pass=0 fail=0 elapsed_ms=0
  if [ -s "$RESULTS_FILE" ]; then
    read -r pass fail elapsed_ms < "$RESULTS_FILE"
  fi

  print_summary "${pass:-0}" "${fail:-0}" "${elapsed_ms:-0}"

  if [ "$typecheck_ok" = false ] || [ "${fail:-0}" -gt 0 ]; then
    exit 1
  fi
}

main
