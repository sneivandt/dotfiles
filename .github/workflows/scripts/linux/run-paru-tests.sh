#!/bin/sh
# shellcheck disable=SC3043,SC2154,SC2030,SC2031
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# run-paru-tests.sh
# -----------------------------------------------------------------------------
# Consolidated paru test runner for CI.
# Runs all paru tests in a single execution to reduce overhead.
# -----------------------------------------------------------------------------

REPO_DIR="$1"

cd "$REPO_DIR"
DIR="$REPO_DIR"
export DIR
export OPT=""

. ./src/linux/logger.sh
. ./src/linux/utils.sh
. ./.github/workflows/scripts/linux/test-paru.sh

# Run all paru tests in sequence
test_paru_prerequisites
test_paru_install
test_paru_available
test_aur_packages
test_paru_config
test_paru_idempotency

echo "All paru tests completed successfully"
