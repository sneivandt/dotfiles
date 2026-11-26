#!/bin/sh
#
# Shared staged-tree export and hook-mode helpers sourced by check-rust.sh and
# check-ci-guards.sh. Keeping this in a .sh helper ensures InstallGitHooks does
# not install it as a standalone Git hook.

init_staged_tree_context() {
  # shellcheck disable=SC2034
  if git rev-parse --verify HEAD >/dev/null 2>&1; then
    against=HEAD
  else
    against=$(git hash-object -t tree /dev/null)
  fi

  WORKTREE_ROOT=$(git rev-parse --show-toplevel)
  REPO_ROOT="$WORKTREE_ROOT"
  # shellcheck disable=SC2034
  MANIFEST="$REPO_ROOT/cli/Cargo.toml"
  OWN_CHECK_STAGE_DIR=""
  CHECK_STAGE_DIR=""
  CHECK_ROOT=""
  HOOK_CARGO_TARGET_DIR=""
  trap '[ -z "$OWN_CHECK_STAGE_DIR" ] || rm -rf "$OWN_CHECK_STAGE_DIR"' EXIT
  trap 'exit 1' HUP INT TERM
}

prepare_staged_tree() {
  [ -z "$CHECK_ROOT" ] || return 0
  if [ -n "${DOTFILES_HOOK_CHECK_ROOT:-}" ]; then
    CHECK_STAGE_DIR="$DOTFILES_HOOK_CHECK_ROOT"
  else
    OWN_CHECK_STAGE_DIR=$(mktemp -d)
    CHECK_STAGE_DIR="$OWN_CHECK_STAGE_DIR"
  fi
  CHECK_ROOT="$CHECK_STAGE_DIR/tree"
  if [ ! -f "$CHECK_STAGE_DIR/.exported" ]; then
    mkdir -p "$CHECK_ROOT"
    git -C "$WORKTREE_ROOT" checkout-index --all --prefix="$CHECK_ROOT/"
    : > "$CHECK_STAGE_DIR/.exported"
  fi
  HOOK_CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$WORKTREE_ROOT/cli/target/hooks"}
  case "$HOOK_CARGO_TARGET_DIR" in
    /*|?:/*|?:\\*) ;;
    *) HOOK_CARGO_TARGET_DIR="$WORKTREE_ROOT/cli/$HOOK_CARGO_TARGET_DIR" ;;
  esac
  REPO_ROOT="$CHECK_ROOT"
  # shellcheck disable=SC2034
  MANIFEST="$REPO_ROOT/cli/Cargo.toml"
}

full_checks_enabled() {
  case "${DOTFILES_HOOKS_FULL:-0}" in
    1 | true | yes) return 0 ;;
    *) return 1 ;;
  esac
}
