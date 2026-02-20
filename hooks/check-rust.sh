#!/bin/sh
#
# Runs Rust code quality checks on staged Rust files:
#   1. cargo fmt --check  (formatting)
#   2. cargo clippy       (linting)
#
# Can be run standalone or called from the pre-commit hook.
# Usage: sh check-rust.sh

set -o errexit
set -o nounset

RED=$(printf '\033[0;31m')
YELLOW=$(printf '\033[1;33m')
NC=$(printf '\033[0m')

if git rev-parse --verify HEAD >/dev/null 2>&1; then
  against=HEAD
else
  against=$(git hash-object -t tree /dev/null)
fi

if git diff --cached --name-only --diff-filter=ACM "$against" | grep -q '\.rs$'; then
  MANIFEST=$(git rev-parse --show-toplevel)/cli/Cargo.toml

  printf "Running cargo fmt --check...\n"
  if ! cargo fmt --manifest-path "$MANIFEST" --check 2>&1; then
    printf '\n%s======================================================%s\n' "$RED" "$NC"
    printf '%sCommit aborted: Rust files are not formatted.%s\n' "$RED" "$NC"
    printf '%sRun the following to fix:%s\n' "$YELLOW" "$NC"
    printf "  cargo fmt --manifest-path cli/Cargo.toml\n"
    printf '%sor bypass with:%s\n' "$YELLOW" "$NC"
    printf "  git commit --no-verify\n\n"
    exit 1
  fi

  printf "Running cargo clippy...\n"
  if ! cargo clippy --manifest-path "$MANIFEST" --all-targets -- -D warnings 2>&1; then
    printf '\n%s======================================================%s\n' "$RED" "$NC"
    printf '%sCommit aborted: cargo clippy reported warnings.%s\n' "$RED" "$NC"
    printf '%sFix the issues above or use:%s\n' "$YELLOW" "$NC"
    printf "  git commit --no-verify\n\n"
    exit 1
  fi
fi

exit 0
