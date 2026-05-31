#!/bin/sh
#
# Runs fast targeted local checks for files that have caused recent CI failures.
# This complements check-rust.sh, which owns Rust fmt/clippy/test checks.
#
# Full-mode checks run only when DOTFILES_HOOKS_FULL=1 so ordinary commits stay
# fast.
#
# Can be run standalone or called from the pre-commit hook.
# Usage: sh check-ci-guards.sh

set -o errexit
set -o nounset

RED=$(printf '\033[0;31m')
YELLOW=$(printf '\033[1;33m')
DIM=$(printf '\033[2m')
NC=$(printf '\033[0m')

if git rev-parse --verify HEAD >/dev/null 2>&1; then
  against=HEAD
else
  against=$(git hash-object -t tree /dev/null)
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
STAGED=$(git diff --cached --name-only --diff-filter=ACM "$against")
MANIFEST="$REPO_ROOT/cli/Cargo.toml"

full_checks_enabled() {
  case "${DOTFILES_HOOKS_FULL:-0}" in
    1 | true | yes) return 0 ;;
    *) return 1 ;;
  esac
}

has_staged_match() {
  pattern="$1"
  printf '%s\n' "$STAGED" | grep -Eq "$pattern"
}

staged_shell_files() {
  printf '%s\n' "$STAGED" \
    | grep -E '(^dotfiles\.sh$|^install\.sh$|\.sh$|^hooks/pre-commit$|^hooks/[^/]+\.sh$)' \
    | while IFS= read -r file; do
        [ -f "$REPO_ROOT/$file" ] && printf '%s\n' "$REPO_ROOT/$file"
      done
}

abort_with_hint() {
  message="$1"
  hint="$2"
  printf '\n%s======================================================%s\n' "$RED" "$NC"
  printf '%sCommit aborted: %s%s\n' "$RED" "$message" "$NC"
  printf '%sRun:%s\n' "$YELLOW" "$NC"
  printf "  %s\n" "$hint"
  printf '%sor bypass with:%s\n' "$YELLOW" "$NC"
  printf "  git commit --no-verify\n\n"
  exit 1
}

run_config_validation() {
  printf "Running config reference validation...\n"
  export DIR="$REPO_ROOT"
  scripts_dir="$REPO_ROOT/.github/workflows/scripts/linux"
  if ! (
    cd "$scripts_dir"
    rc=0
    sh test-config.sh config_validation    || rc=1
    sh test-config.sh symlinks_validation  || rc=1
    sh test-config.sh chmod_validation     || rc=1
    sh test-config.sh toml_syntax          || rc=1
    sh test-config.sh category_consistency || rc=1
    sh test-config.sh empty_sections       || rc=1
    exit "$rc"
  ); then
    abort_with_hint \
      "configuration validation failed." \
      "cd .github/workflows/scripts/linux && DIR=\"\$(git rev-parse --show-toplevel)\" sh test-config.sh symlinks_validation"
  fi

  if full_checks_enabled && command -v cargo >/dev/null 2>&1; then
    printf "Running config drift integration test...\n"
    if ! cargo test --profile ci --manifest-path "$MANIFEST" --test config_drift 2>&1; then
      abort_with_hint \
        "config drift tests failed." \
        "cargo test --profile ci --manifest-path cli/Cargo.toml --test config_drift"
    fi
  elif full_checks_enabled; then
    printf '%sSkipping config drift integration test: cargo not installed.%s\n' "$DIM" "$NC"
  else
    printf '%sSkipping config drift integration test: set DOTFILES_HOOKS_FULL=1 to run it.%s\n' "$DIM" "$NC"
  fi
}

run_dependency_guards() {
  if grep -nE '=[[:space:]]*"\*"' "$MANIFEST"; then
    abort_with_hint \
      "Cargo.toml contains a wildcard dependency." \
      "replace wildcard dependency versions in cli/Cargo.toml"
  fi

  if full_checks_enabled && cargo deny --version >/dev/null 2>&1; then
    printf "Running cargo-deny bans/licenses/sources checks...\n"
    if ! cargo deny --manifest-path "$MANIFEST" check bans licenses sources 2>&1; then
      abort_with_hint \
        "cargo-deny reported dependency policy violations." \
        "cargo deny --manifest-path cli/Cargo.toml check bans licenses sources"
    fi
  elif full_checks_enabled; then
    printf '%sSkipping cargo-deny: cargo-deny not installed.%s\n' "$DIM" "$NC"
    printf '%s  Install: cargo install cargo-deny --locked%s\n' "$DIM" "$NC"
  else
    printf '%sSkipping cargo-deny: set DOTFILES_HOOKS_FULL=1 to run it.%s\n' "$DIM" "$NC"
  fi
}

run_shell_guards() {
  if command -v shellcheck >/dev/null 2>&1; then
    printf "Running ShellCheck...\n"
    shell_files="$(staged_shell_files)"
    if [ -z "$shell_files" ]; then
      return 0
    fi
    # shellcheck disable=SC2086  # intentional word splitting of newline-free paths
    if ! shellcheck --severity=warning --shell=sh $shell_files; then
      abort_with_hint \
        "ShellCheck reported issues." \
        "shellcheck --severity=warning --shell=sh <staged shell files>"
    fi
  else
    printf '%sSkipping ShellCheck: shellcheck not installed.%s\n' "$DIM" "$NC"
  fi
}

run_wrapper_guards() {
  printf "Running Linux shell wrapper tests...\n"
  export DIR="$REPO_ROOT"
  if [ -z "${BINARY_PATH:-}" ]; then
    if [ -x "$REPO_ROOT/cli/target/dev-opt/dotfiles" ]; then
      BINARY_PATH="$REPO_ROOT/cli/target/dev-opt/dotfiles"
    elif [ -x "$REPO_ROOT/cli/target/ci/dotfiles" ]; then
      BINARY_PATH="$REPO_ROOT/cli/target/ci/dotfiles"
    else
      BINARY_PATH=""
    fi
  fi
  export BINARY_PATH
  scripts_dir="$REPO_ROOT/.github/workflows/scripts/linux"
  if ! (
    cd "$scripts_dir"
    sh test-shell-wrapper.sh
  ); then
    abort_with_hint \
      "Linux shell wrapper tests failed." \
      "cd .github/workflows/scripts/linux && DIR=\"\$(git rev-parse --show-toplevel)\" BINARY_PATH=\"\" sh test-shell-wrapper.sh"
  fi
}

if has_staged_match '^(conf/.*\.toml|symlinks/)'; then
  run_config_validation
fi

if has_staged_match '^(cli/Cargo\.toml|cli/Cargo\.lock|cli/deny\.toml)$'; then
  run_dependency_guards
fi

if has_staged_match '(^dotfiles\.sh$|\.sh$|^hooks/)'; then
  run_shell_guards
fi

if full_checks_enabled && has_staged_match '(^dotfiles\.sh$|^\.github/workflows/scripts/linux/test-shell-wrapper\.sh$)'; then
  run_wrapper_guards
elif has_staged_match '(^dotfiles\.sh$|^\.github/workflows/scripts/linux/test-shell-wrapper\.sh$)'; then
  printf '%sSkipping Linux shell wrapper tests: set DOTFILES_HOOKS_FULL=1 to run them.%s\n' "$DIM" "$NC"
fi

exit 0
