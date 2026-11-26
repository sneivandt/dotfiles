#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-shell-wrapper.sh — Tests for dotfiles.sh wrapper
# Usage: sh tests/wrapper/test-shell-wrapper.sh [test_name...]
# -----------------------------------------------------------------------------

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
DIR=${DIR:-$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)}
# shellcheck source=../lib/test-helpers.sh
. "$SCRIPT_DIR/../lib/test-helpers.sh"
# shellcheck source=linux/test-args-and-build.sh
. "$SCRIPT_DIR/linux/test-args-and-build.sh"
# shellcheck source=linux/test-bootstrap.sh
. "$SCRIPT_DIR/linux/test-bootstrap.sh"
# shellcheck source=linux/test-attestation.sh
. "$SCRIPT_DIR/linux/test-attestation.sh"

case "$0" in
  *test-shell-wrapper.sh)
    if [ "$#" -gt 0 ]; then
      for test_name in "$@"; do
        "$test_name"
      done
      exit 0
    fi
    test_wrapper_build_mode
    test_wrapper_uses_local_binary
    test_wrapper_forwarded_args
    test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args
    WRAPPER_BOOTSTRAP_SUFFIX=' path with \ backslash' test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args
    WRAPPER_EMPTY_CACHE=1 test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args
    WRAPPER_BLOCK_PUBLISH=1 test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args
    test_wrapper_recovers_truncated_cli
    test_wrapper_release_metadata_formats
    test_wrapper_failed_bootstrap_preserves_cached_binary
    test_wrapper_build_mode_consumes_build_flag_and_forwards_cli_args
    test_wrapper_forwards_advanced_flags
    test_wrapper_preserves_runtime_context
    test_wrapper_uses_cargo_artifact
    test_wrapper_chmod_after_checksum
    test_wrapper_attestation_verification
    test_wrapper_release_pinned_urls
    test_wrapper_release_repository
    echo "All shell wrapper tests passed"
    ;;
esac
