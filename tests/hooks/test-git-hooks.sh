#!/bin/sh
#
# CI test for pre-commit hook - verifies sensitive pattern detection,
# hook wiring/modes, release workflow guards, and pre-push protection.

set -o errexit
set -o nounset

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
export REPO_ROOT="${DIR:-$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)}"

# Run commit-creating hook tests inside a disposable temporary repository so
# this suite never mutates the caller's checkout or sweeps up uncommitted work.
HOOK_TEST_REPO=$(mktemp -d)
trap 'rm -rf "$HOOK_TEST_REPO"' EXIT

# shellcheck source=git/helpers.sh
. "$SCRIPT_DIR/git/helpers.sh"
# shellcheck source=git/test-hook-behavior.sh
. "$SCRIPT_DIR/git/test-hook-behavior.sh"
# shellcheck source=git/test-sensitive-scan.sh
. "$SCRIPT_DIR/git/test-sensitive-scan.sh"

init_hook_test_repo

printf "Testing pre-commit hook sensitive pattern detection\n"
printf "==================================================\n\n"

printf "Testing hook wiring...\n"
test_hook_wiring

printf "Testing hook modes...\n"
test_hook_modes

printf "Testing release workflow guards...\n"
test_release_workflow_guards

printf "Testing hook portability...\n"
test_sensitive_scan_without_paste
test_sensitive_scan_unusual_paths
test_sensitive_scan_errors

printf "Testing staged and whitespace-safe hook inputs...\n"
sh "$SCRIPT_DIR/test-hook-inputs.sh"

printf "Testing pre-push protection...\n"
test_pre_push_protection
python3 -B "$SCRIPT_DIR/test-maintenance.py"

run_sensitive_pattern_tests

# Summary
printf "\n==================================================\n"
printf "Test Results:\n"
printf "  Passed: %d\n" "$tests_passed"
printf "  Failed: %d\n" "$tests_failed"

if [ "$tests_failed" -gt 0 ]; then
  printf "\n✗ Some tests failed!\n"
  exit 1
else
  printf "\n✓ All tests passed!\n"
  exit 0
fi
