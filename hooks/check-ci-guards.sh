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
WORKTREE_ROOT="$REPO_ROOT"
MANIFEST="$REPO_ROOT/cli/Cargo.toml"
CHECK_ROOT=""
trap '[ -z "$CHECK_ROOT" ] || rm -rf "$CHECK_ROOT"' EXIT
trap 'exit 1' HUP INT TERM

prepare_staged_tree() {
  [ -z "$CHECK_ROOT" ] || return 0
  CHECK_ROOT=$(mktemp -d)
  git -C "$REPO_ROOT" checkout-index --all --prefix="$CHECK_ROOT/"
  HOOK_CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$REPO_ROOT/cli/target/hooks"}
  case "$HOOK_CARGO_TARGET_DIR" in
    /*|?:/*|?:\\*) ;;
    *) HOOK_CARGO_TARGET_DIR="$REPO_ROOT/cli/$HOOK_CARGO_TARGET_DIR" ;;
  esac
  REPO_ROOT="$CHECK_ROOT"
  MANIFEST="$REPO_ROOT/cli/Cargo.toml"
}

run_cargo() {
  (
    cd "$REPO_ROOT/cli" &&
      CARGO_TARGET_DIR="$HOOK_CARGO_TARGET_DIR" \
      DOTFILES_VERSION="${DOTFILES_VERSION:-dev-hook}" \
      cargo "$@"
  )
}

full_checks_enabled() {
  case "${DOTFILES_HOOKS_FULL:-0}" in
    1 | true | yes) return 0 ;;
    *) return 1 ;;
  esac
}

has_staged_match() {
  # Filter before Git quotes display filenames. Include deletions and both
  # sides of renames when deciding which checks a commit needs.
  changed=$(git diff --cached --name-only --no-renames "$against" -- "$@") || exit 1
  [ -n "$changed" ]
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
  if ! command -v cargo >/dev/null 2>&1; then
    printf '%sSkipping typed config validation: cargo not installed.%s\n' "$DIM" "$NC"
    return
  fi

  prepare_staged_tree
  if [ -f "$REPO_ROOT/cli/rust-toolchain.toml" ]; then
    sh "$(dirname -- "$0")/check-toolchain.sh" "$REPO_ROOT/cli"
  fi
  printf "Running typed config validation...\n"
  if ! run_cargo run --locked --quiet --profile ci --manifest-path "$MANIFEST" -- \
    check --root "$REPO_ROOT" -p desktop \
    --only config-diagnostics,symlink-sources,config-files 2>&1; then
    abort_with_hint \
      "configuration validation failed." \
      "cd cli && cargo run --locked --profile ci -- check --root .. -p desktop --only config-diagnostics,symlink-sources,config-files"
  fi

  if full_checks_enabled; then
    printf "Running config drift integration test...\n"
    if ! run_cargo test --locked --profile ci --manifest-path "$MANIFEST" --test config_drift 2>&1; then
      abort_with_hint \
        "config drift tests failed." \
        "cd cli && cargo test --locked --profile ci --test config_drift"
    fi
  else
    printf '%sSkipping config drift integration test: set DOTFILES_HOOKS_FULL=1 to run it.%s\n' "$DIM" "$NC"
  fi
}

run_dependency_guards() {
  prepare_staged_tree
  if grep -nE '=[[:space:]]*"\*"' "$MANIFEST"; then
    abort_with_hint \
      "Cargo.toml contains a wildcard dependency." \
      "replace wildcard dependency versions in cli/Cargo.toml"
  fi

  if full_checks_enabled && run_cargo deny --version >/dev/null 2>&1; then
    printf "Running cargo-deny bans/licenses/sources checks...\n"
    if ! run_cargo deny --manifest-path "$MANIFEST" check bans licenses sources 2>&1; then
      abort_with_hint \
        "cargo-deny reported dependency policy violations." \
        "cd cli && cargo deny check bans licenses sources"
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
    prepare_staged_tree
    printf "Running ShellCheck...\n"
    git diff --cached --name-only --no-renames --diff-filter=d -z "$against" -- \
      '*.sh' '*.bash' hooks/pre-commit symlinks/bashrc symlinks/bash_profile \
      'symlinks/config/git/templates/hooks/*' 'symlinks/config/git/templates/hooks.local/*' \
      'packages/*/PKGBUILD' 'packages/*/*.install' > "$CHECK_ROOT/.shell-inputs"
    if [ ! -s "$CHECK_ROOT/.shell-inputs" ]; then
      return 0
    fi
    analyzer="$(CDPATH='' cd -- "$(dirname -- "$0")/../cli/src/app/validation/scripts" && pwd)/shellcheck.sh"
    if ! (cd "$CHECK_ROOT" && xargs -0 sh -c '
      analyzer=$1
      shift
      for file do
        set -- "$@" "./$file"
        shift
      done
      exec sh "$analyzer" "$@"
    ' sh "$analyzer" < .shell-inputs); then
      abort_with_hint \
        "ShellCheck reported issues." \
        "sh cli/src/app/validation/scripts/shellcheck.sh <staged shell files>"
    fi
  else
    printf '%sSkipping ShellCheck: shellcheck not installed.%s\n' "$DIM" "$NC"
  fi
}

run_wrapper_guards() {
  prepare_staged_tree
  printf "Running Linux shell wrapper tests...\n"
  export DIR="$REPO_ROOT"
  if [ -z "${BINARY_PATH:-}" ]; then
    if [ -x "$WORKTREE_ROOT/cli/target/dev-opt/dotfiles" ]; then
      BINARY_PATH="$WORKTREE_ROOT/cli/target/dev-opt/dotfiles"
    elif [ -x "$WORKTREE_ROOT/cli/target/ci/dotfiles" ]; then
      BINARY_PATH="$WORKTREE_ROOT/cli/target/ci/dotfiles"
    else
      BINARY_PATH=""
    fi
  fi
  export BINARY_PATH
  if ! sh "$REPO_ROOT/tests/wrapper/test-shell-wrapper.sh"; then
    abort_with_hint \
      "Linux shell wrapper tests failed." \
      "DIR=\"\$(git rev-parse --show-toplevel)\" BINARY_PATH=\"\" sh tests/wrapper/test-shell-wrapper.sh"
  fi
}

run_release_workflow_guards() {
  if ! command -v python3 >/dev/null 2>&1; then
    printf '%sSkipping release workflow guards: python3 not installed.%s\n' "$DIM" "$NC"
    return
  fi

  printf "Running release workflow guards...\n"

  if ! workflow_contents=$(git show ":.github/workflows/release.yml"); then
    abort_with_hint \
      "staged release workflow cannot be read." \
      "stage .github/workflows/release.yml before running the publishing guards"
  fi

  contract="$(CDPATH='' cd -- "$(dirname -- "$0")/../.github/workflows/scripts/linux" && pwd)/check-ci-contract.py"
  if ! printf '%s\n' "$workflow_contents" | python3 -B "$contract" --release - 2>&1; then
    abort_with_hint \
      "release workflow contract check failed." \
      "python3 -B .github/workflows/scripts/linux/check-ci-contract.py"
  fi
}

if has_staged_match 'conf/*.toml' 'symlinks/*' 'system/*' 'packages/*'; then
  run_config_validation
fi

if has_staged_match cli/Cargo.toml cli/Cargo.lock cli/deny.toml; then
  run_dependency_guards
fi

if has_staged_match '*.sh' '*.bash' hooks/pre-commit symlinks/bashrc symlinks/bash_profile \
  'symlinks/config/git/templates/hooks/*' 'symlinks/config/git/templates/hooks.local/*' \
  'packages/*/PKGBUILD' 'packages/*/*.install'; then
  run_shell_guards
fi

if full_checks_enabled && has_staged_match dotfiles.sh tests/wrapper/test-shell-wrapper.sh 'tests/wrapper/linux/*'; then
  run_wrapper_guards
elif has_staged_match dotfiles.sh tests/wrapper/test-shell-wrapper.sh 'tests/wrapper/linux/*'; then
  printf '%sSkipping Linux shell wrapper tests: set DOTFILES_HOOKS_FULL=1 to run them.%s\n' "$DIM" "$NC"
fi

if has_staged_match .github/workflows/release.yml; then
  run_release_workflow_guards
fi

exit 0
