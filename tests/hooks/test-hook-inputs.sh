#!/bin/sh
set -o errexit
set -o nounset

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
DIR=${DIR:-$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)}
# shellcheck source=../lib/test-helpers.sh
. "$SCRIPT_DIR/../lib/test-helpers.sh"
# shellcheck source=inputs/helpers.sh
. "$SCRIPT_DIR/inputs/helpers.sh"
# shellcheck source=inputs/test-staged-guards.sh
. "$SCRIPT_DIR/inputs/test-staged-guards.sh"
# shellcheck source=inputs/test-allowlists.sh
. "$SCRIPT_DIR/inputs/test-allowlists.sh"
# shellcheck source=inputs/test-build-triggers.sh
. "$SCRIPT_DIR/inputs/test-build-triggers.sh"

if [ "$#" -gt 0 ]; then
  for test_name do
    "$test_name"
  done
  exit 0
fi

test_rust_and_shellcheck_inputs
test_fixture_git_context
test_staged_ci_guards
test_staged_ci_guard_deletions
test_build_version_ref_triggers
test_github_ssh_allowlist
test_shell_sha256_case_allowlist
test_pkgbuild_checksum_allowlist
test_sensitive_posix_patterns
test_staged_quoted_shell
test_staged_quoted_powershell
test_staged_python_and_lua_guards
