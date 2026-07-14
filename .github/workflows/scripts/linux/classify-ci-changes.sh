#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# classify-ci-changes.sh — Decide which CI jobs are relevant for the current diff.
# Dependencies: test-helpers.sh
# Expected:     DIR, GITHUB_OUTPUT, GITHUB_EVENT_NAME, BASE_SHA, HEAD_SHA
# -----------------------------------------------------------------------------

# shellcheck disable=SC3054
# When sourced with `.`, use BASH_SOURCE if available (bash); otherwise use pwd
if [ -n "${BASH_SOURCE:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
else
  SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
fi
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

emit_output() {
  printf "%s=%s\n" "$1" "$2" >> "$GITHUB_OUTPUT"
}

write_outputs() {
  emit_output docs_only "$1"
  emit_output run_lint "$2"
  emit_output run_validate_config "$3"
  emit_output run_rust_checks "$4"
  emit_output run_build_artifacts "$5"
  emit_output run_profile_integration "$6"
  emit_output run_app_tests "$7"
  emit_output run_git_hooks "$8"
  emit_output run_wrapper_linux "$9"
  emit_output run_wrapper_windows "${10}"
}

write_full_outputs() {
  write_outputs false true true true true true true true true true
}

write_docs_only_outputs() {
  write_outputs true false false false false false false false false false
}

if [ -z "${DIR:-}" ] || [ -z "${GITHUB_OUTPUT:-}" ]; then
  log_error "DIR and GITHUB_OUTPUT must be set"
fi

if [ "${GITHUB_EVENT_NAME:-}" = "workflow_dispatch" ]; then
  log_stage "Manual workflow dispatch: running full CI"
  write_full_outputs
  exit 0
fi

if [ -z "${BASE_SHA:-}" ] || [ -z "${HEAD_SHA:-}" ]; then
  log_stage "No comparison range available: running full CI"
  write_full_outputs
  exit 0
fi

changed_files_file="$(mktemp)"
trap 'rm -f "$changed_files_file"' EXIT HUP INT TERM

git -C "$DIR" diff --name-only "$BASE_SHA" "$HEAD_SHA" > "$changed_files_file"

if [ ! -s "$changed_files_file" ]; then
  log_stage "No changed files detected: running full CI"
  write_full_outputs
  exit 0
fi

log_stage "Changed files"
while IFS= read -r changed_file || [ -n "$changed_file" ]; do
  printf " - %s\n" "$changed_file"
done < "$changed_files_file"

docs_only=1
run_full=0
rust_related=0
config_related=0
hook_related=0
wrapper_linux_related=0
wrapper_windows_related=0

while IFS= read -r changed_file || [ -n "$changed_file" ]; do
  case "$changed_file" in
    docs/*|.agents/*|*.md)
      ;;
    .github/workflows/*)
      docs_only=0
      run_full=1
      ;;
    cli/*|rust-toolchain.toml)
      docs_only=0
      rust_related=1
      ;;
    conf/*|symlinks/*)
      docs_only=0
      config_related=1
      ;;
    hooks/*)
      docs_only=0
      hook_related=1
      ;;
    dotfiles.sh|install.sh)
      docs_only=0
      wrapper_linux_related=1
      ;;
    dotfiles.ps1)
      docs_only=0
      wrapper_windows_related=1
      ;;
    *)
      docs_only=0
      run_full=1
      ;;
  esac
done < "$changed_files_file"

if [ "$docs_only" -eq 1 ]; then
  log_stage "Docs-only or agent-doc-only change: skipping expensive CI jobs"
  write_docs_only_outputs
  exit 0
fi

if [ "$run_full" -eq 1 ]; then
  log_stage "Workflow or uncategorized change: running full CI"
  write_full_outputs
  exit 0
fi

run_build_artifacts=0
run_profile_integration=0
run_app_tests=0
run_wrapper_linux=0
run_wrapper_windows=0

if [ "$rust_related" -eq 1 ] || [ "$config_related" -eq 1 ] || [ "$wrapper_linux_related" -eq 1 ] || [ "$wrapper_windows_related" -eq 1 ]; then
  run_build_artifacts=1
fi

if [ "$rust_related" -eq 1 ] || [ "$config_related" -eq 1 ]; then
  run_profile_integration=1
  run_app_tests=1
fi

if [ "$rust_related" -eq 1 ] || [ "$wrapper_linux_related" -eq 1 ]; then
  run_wrapper_linux=1
fi

if [ "$rust_related" -eq 1 ] || [ "$wrapper_windows_related" -eq 1 ]; then
  run_wrapper_windows=1
fi

write_outputs \
  false \
  true \
  true \
  "$( [ "$rust_related" -eq 1 ] && echo true || echo false )" \
  "$( [ "$run_build_artifacts" -eq 1 ] && echo true || echo false )" \
  "$( [ "$run_profile_integration" -eq 1 ] && echo true || echo false )" \
  "$( [ "$run_app_tests" -eq 1 ] && echo true || echo false )" \
  "$( [ "$hook_related" -eq 1 ] && echo true || echo false )" \
  "$( [ "$run_wrapper_linux" -eq 1 ] && echo true || echo false )" \
  "$( [ "$run_wrapper_windows" -eq 1 ] && echo true || echo false )"
