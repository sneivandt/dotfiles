#!/bin/sh
#
# Runs Rust code quality checks on staged files:
#   1. cargo fmt --check                       (formatting, on .rs changes)
#   2. cargo clippy (host target)              (linting, on .rs changes)
#   3. cargo clippy --target x86_64-pc-windows-gnu
#                                              (cross-platform linting, full mode)
#   4. cargo test                              (unit/integration tests, full mode)
#
# Full-mode checks run only when DOTFILES_HOOKS_FULL=1 so ordinary commits stay
# fast. Non-Rust linters (including PSScriptAnalyzer) live in check-ci-guards.sh.
#
# Can be run standalone or called from the pre-commit hook.
# Usage: sh check-rust.sh

set -o errexit
set -o nounset

RED=$(printf '\033[0;31m')
YELLOW=$(printf '\033[1;33m')
DIM=$(printf '\033[2m')
NC=$(printf '\033[0m')

WIN_TARGET="x86_64-pc-windows-gnu"
SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=hooks/staged-tree.sh
. "$SCRIPT_DIR/staged-tree.sh"

run_cargo() {
  (cd "$CLI_ROOT" && cargo "$@")
}

run_rustc() {
  (cd "$CLI_ROOT" && rustc "$@")
}

init_staged_tree_context

# Filter in Git so quoted display filenames never affect extension detection.
# Deletions and renames away from .rs can also break the crate.
RUST_CHANGED=$(git diff --cached --name-only --no-renames "$against" -- \
  '*.rs' cli/Cargo.toml cli/Cargo.lock \
  cli/rust-toolchain.toml cli/rust-toolchain rust-toolchain.toml rust-toolchain \
  cli/.cargo/config.toml cli/.cargo/config .cargo/config.toml .cargo/config)

# Check an exported index rather than temporarily replacing the working tree.
if [ -z "$RUST_CHANGED" ]; then
  exit 0
fi
prepare_staged_tree
CARGO_TARGET_DIR="$HOOK_CARGO_TARGET_DIR"
DOTFILES_VERSION="${DOTFILES_VERSION:-dev-hook}"
export CARGO_TARGET_DIR DOTFILES_VERSION

# ── Rust checks ────────────────────────────────────────
CLI_ROOT="$CHECK_ROOT/cli"
sh "$SCRIPT_DIR/check-toolchain.sh" "$CLI_ROOT"

printf "Running cargo fmt --check...\n"
if ! run_cargo fmt --manifest-path "$MANIFEST" --check 2>&1; then
  printf '\n%s======================================================%s\n' "$RED" "$NC"
  printf '%sCommit aborted: Rust files are not formatted.%s\n' "$RED" "$NC"
  printf '%sRun the following to fix:%s\n' "$YELLOW" "$NC"
  printf "  cd cli && cargo fmt\n"
  printf '%sor bypass with:%s\n' "$YELLOW" "$NC"
  printf "  git commit --no-verify\n\n"
  exit 1
fi

printf "Running cargo clippy (host)...\n"
if ! run_cargo clippy --locked --profile ci --manifest-path "$MANIFEST" --all-targets -- -D warnings 2>&1; then
  printf '\n%s======================================================%s\n' "$RED" "$NC"
  printf '%sCommit aborted: cargo clippy reported warnings.%s\n' "$RED" "$NC"
  printf '%sFix the issues above or use:%s\n' "$YELLOW" "$NC"
  printf "  git commit --no-verify\n\n"
  exit 1
fi

if full_checks_enabled; then
  # Cross-target clippy: catches Windows-only cfg arms, missing imports under
  # #[cfg(windows)], winreg type errors, etc. Skipped when toolchain absent.
  if run_rustc --print target-list 2>/dev/null | grep -qx "$WIN_TARGET" \
     && run_rustc --print sysroot 2>/dev/null | xargs -I{} test -d "{}/lib/rustlib/$WIN_TARGET"; then
    printf "Running cargo clippy (%s)...\n" "$WIN_TARGET"
    if ! run_cargo clippy --locked --profile ci --manifest-path "$MANIFEST" --target "$WIN_TARGET" --all-targets -- -D warnings 2>&1; then
      printf '\n%s======================================================%s\n' "$RED" "$NC"
      printf '%sCommit aborted: cargo clippy reported warnings on %s.%s\n' "$RED" "$WIN_TARGET" "$NC"
      printf '%sFix the issues above or use:%s\n' "$YELLOW" "$NC"
      printf "  git commit --no-verify\n\n"
      exit 1
    fi
  else
    printf '%sSkipping cross-target clippy (%s): toolchain not installed.%s\n' "$DIM" "$WIN_TARGET" "$NC"
    printf '%s  Install: cd cli && rustup target add %s && pacman -S mingw-w64-gcc%s\n' "$DIM" "$WIN_TARGET" "$NC"
  fi

  printf "Running cargo test...\n"
  if ! run_cargo test --locked --profile ci --manifest-path "$MANIFEST" 2>&1; then
    printf '\n%s======================================================%s\n' "$RED" "$NC"
    printf '%sCommit aborted: cargo test failed.%s\n' "$RED" "$NC"
    printf '%sFix the issues above or use:%s\n' "$YELLOW" "$NC"
    printf "  git commit --no-verify\n\n"
    exit 1
  fi
else
  printf '%sSkipping full Rust hook checks: set DOTFILES_HOOKS_FULL=1 to run Windows clippy and cargo test.%s\n' "$DIM" "$NC"
fi

exit 0
