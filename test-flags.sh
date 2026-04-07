#!/bin/bash

# Test script for all kuizer flags
# Tests: flag existence, help text, empty value validation, and basic functionality

set -e

KUIZER="./dist/index.js"
FAILED_TESTS=()
TOTAL_TESTS=0

# Test helper functions
test_flag() {
    local flag="$1"
    local description="$2"
    local test_command="$3"
    local expected_exit_code="${4:-0}"

    TOTAL_TESTS=$((TOTAL_TESTS + 1))
    echo "Testing $flag: $description"

    if eval "$test_command" >/dev/null 2>&1; then
        actual_exit_code=$?
    else
        actual_exit_code=$?
    fi

    if [ $actual_exit_code -eq $expected_exit_code ]; then
        echo "  ✓ Pass"
    else
        echo "  ✗ Fail (expected exit code $expected_exit_code, got $actual_exit_code)"
        FAILED_TESTS+=("$flag: $description")
    fi
    echo
}

# Start testing
echo "=== Testing Kuizer Flags ==="
echo "Build: $(bun run build >/dev/null 2>&1 && echo 'Success' || echo 'Failed')"
echo

# Test help flag
test_flag "--help" "Help menu" "$KUIZER --help" 0
test_flag "-h" "Short help" "$KUIZER -h" 0
test_flag "help" "Help command" "$KUIZER help" 0

# Test version flag
test_flag "--version" "Version flag" "$KUIZER --version" 0
test_flag "-v" "Short version" "$KUIZER -v" 0
test_flag "version" "Version command" "$KUIZER version" 0

# Test config flag
test_flag "--config" "Config help" "$KUIZER config --help" 0
test_flag "-c" "Short config help" "$KUIZER -c --help" 0
test_flag "--config-empty" "Config empty validation" "$KUIZER -c" 1
test_flag "--config-valid" "Config with valid file" "$KUIZER -c package.json" 0

# Test tsconfig flag
test_flag "--tsconfig" "Tsconfig help" "$KUIZER tsconfig --help" 0
test_flag "--tsconfig-empty" "Tsconfig empty validation" "$KUIZER --tsconfig" 1
test_flag "--tsconfig-valid" "Tsconfig with valid file" "$KUIZER --tsconfig tsconfig.json" 0

# Test only flag
test_flag "--only" "Only help" "$KUIZER only --help" 0
test_flag "-o" "Short only help" "$KUIZER -o --help" 0
test_flag "--only-empty" "Only empty validation" "$KUIZER -o" 1
test_flag "--only-valid" "Only with valid detector" "$KUIZER -o unusedExports" 0

# Test skip flag
test_flag "--skip" "Skip help" "$KUIZER skip --help" 0
test_flag "-s" "Short skip help" "$KUIZER -s --help" 0
test_flag "--skip-empty" "Skip empty validation" "$KUIZER -s" 1
test_flag "--skip-valid" "Skip with valid detector" "$KUIZER -s unusedFiles" 0

# Test reporter flag
test_flag "--reporter" "Reporter help" "$KUIZER reporter --help" 0
test_flag "-r" "Short reporter help" "$KUIZER -r --help" 0
test_flag "--reporter-empty" "Reporter empty validation" "$KUIZER -r" 1
test_flag "--reporter-valid" "Reporter with valid format" "$KUIZER -r json" 0

# Test json flag
test_flag "--json" "JSON help" "$KUIZER json --help" 0
test_flag "--json-flag" "JSON flag functionality" "$KUIZER --json" 0
test_flag "--json-standalone" "JSON standalone" "$KUIZER json" 0
test_flag "--json-after-d" "JSON after d command" "$KUIZER d json" 0

# Test filter flag
test_flag "--filter" "Filter help" "$KUIZER filter --help" 0
test_flag "-f" "Short filter help" "$KUIZER -f --help" 0
test_flag "--filter-empty" "Filter empty validation" "$KUIZER -f" 1
test_flag "--filter-valid" "Filter with valid type" "$KUIZER -f functions" 0

# Test exclude-pattern flag
test_flag "--exclude-pattern" "Exclude pattern help" "$KUIZER exclude-pattern --help" 0
test_flag "--exclude-pattern-valid" "Exclude pattern with regex" "$KUIZER --exclude-pattern '^_'" 0

# Test interactive flag
test_flag "--interactive" "Interactive help" "$KUIZER interactive --help" 0
test_flag "--interactive-flag" "Interactive flag" "$KUIZER --interactive" 2  # Exits with code 2 when no TTY

# Test dry-run flag
test_flag "--dry-run" "Dry run help" "$KUIZER dry --help" 0
test_flag "--dry" "Dry flag" "$KUIZER dry" 0
test_flag "--dr" "Dry run short" "$KUIZER dr" 1  # Shows help and exits with 1

# Test write flag
test_flag "--write" "Write help" "$KUIZER write --help" 0
test_flag "-w" "Short write help" "$KUIZER -w --help" 0

# Test no-backup flag
test_flag "--no-backup" "No backup help" "$KUIZER no-backup --help" 0

# Test restore command
test_flag "--snapshot" "Snapshot help" "$KUIZER snapshot --help" 0
test_flag "--snapshot-empty" "Snapshot empty validation" "$KUIZER restore --snapshot" 1
test_flag "--detect" "Detect command" "$KUIZER --detect" 0
test_flag "--fix" "Fix command" "$KUIZER --fix" 0
test_flag "--restore" "Restore command" "$KUIZER restore --snapshot invalid" 2  # Exits with 2 when snapshot not found

# Summary
echo "=== Test Summary ==="
echo "Total tests: $TOTAL_TESTS"
if [ ${#FAILED_TESTS[@]} -eq 0 ]; then
    echo "All tests passed! ✓"
    exit 0
else
    echo "Failed tests: ${#FAILED_TESTS[@]}"
    for test in "${FAILED_TESTS[@]}"; do
        echo "  - $test"
    done
    exit 1
fi
