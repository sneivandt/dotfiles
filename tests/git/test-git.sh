#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-git.sh — Git configuration, aliases, behavior, and cleanup tests.
# Usage: sh tests/git/test-git.sh [config] [aliases] [behavior] [clean]
# -----------------------------------------------------------------------------

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
DIR=${DIR:-$(CDPATH='' cd -- "$SCRIPT_DIR/../.." && pwd)}
# shellcheck source=../lib/test-helpers.sh
. "$SCRIPT_DIR/../lib/test-helpers.sh"

test_config()
{(
  is_program_installed "git" || { log_verbose "Skipping: git not installed"; return 0; }
  log_stage "Testing git configuration"

  git --version >/dev/null 2>&1 || { printf "%sERROR: git --version failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  [ -f "$HOME/.config/git/config" ] || { printf "%sERROR: custom git config not installed: %s%s\n" "${RED}" "$HOME/.config/git/config" "${NC}" >&2; return 1; }
  log_verbose "Custom git config found"

  errors=0
  for kv in "init.defaultBranch=main" "pull.rebase=true" "rebase.updateRefs=true" "merge.conflictstyle=zdiff3" "push.default=simple" "push.autoSetupRemote=true" "push.useForceIfIncludes=true" "diff.algorithm=histogram"; do
    key="${kv%%=*}"; expected="${kv#*=}"
    actual="$(git config --get "$key" 2>/dev/null || echo "")"
    if [ "$actual" = "$expected" ]; then
      log_verbose "✓ $key = $actual"
    else
      printf "%sERROR: %s expected '%s', got '%s'%s\n" "${RED}" "$key" "$expected" "$actual" "${NC}" >&2
      errors=$((errors + 1))
    fi
  done
  [ "$errors" -eq 0 ] || return 1
)}

test_aliases()
{(
  is_program_installed "git" || { log_verbose "Skipping: git not installed"; return 0; }
  log_stage "Testing git aliases"
  [ -f "$HOME/.config/git/config" ] || { printf "%sERROR: git config not installed: %s%s\n" "${RED}" "$HOME/.config/git/config" "${NC}" >&2; return 1; }
  [ -f "$HOME/.config/git/aliases" ] || { printf "%sERROR: git aliases not installed: %s%s\n" "${RED}" "$HOME/.config/git/aliases" "${NC}" >&2; return 1; }

  errors=0
  for a in st br lo ci; do
    if git config --get "alias.$a" >/dev/null 2>&1; then
      log_verbose "✓ alias.$a = $(git config --get "alias.$a")"
    else
      printf "%sERROR: alias.%s not defined%s\n" "${RED}" "$a" "${NC}" >&2
      errors=$((errors + 1))
    fi
  done
  [ "$errors" -eq 0 ] || return 1
)}

test_behavior()
{(
  is_program_installed "git" || { log_verbose "Skipping: git not installed"; return 0; }
  log_stage "Testing git behavior"
  [ -f "$HOME/.config/git/config" ] || { printf "%sERROR: git config not installed: %s%s\n" "${RED}" "$HOME/.config/git/config" "${NC}" >&2; return 1; }

  repo="$(mktemp -d)"
  trap 'rm -rf "$repo"' EXIT
  git init "$repo" >/dev/null 2>&1
  cd "$repo"
  git config user.name "CI Test"
  git config user.email "ci@test.local"

  branch="$(git branch --show-current)"
  if [ "$branch" = "main" ]; then
    log_verbose "✓ Default branch is main"
  else
    printf "%sERROR: default branch is '%s', expected 'main'%s\n" "${RED}" "$branch" "${NC}" >&2
    return 1
  fi

  echo test > test.txt && git add test.txt
  git commit -m "Test commit" >/dev/null 2>&1 || { printf "%sERROR: commit failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "✓ Commit created successfully"
)}

test_clean()
{(
  is_program_installed "git" || { log_verbose "Skipping: git not installed"; return 0; }
  log_stage "Testing interactive git cleanup"

  repo="$(mktemp -d)"
  trap 'rm -rf "$repo"' EXIT
  git -c init.templateDir= init -q "$repo"
  cd "$repo"
  git config include.path "$DIR/symlinks/config/git/aliases"
  mkdir -p .git/info
  printf '.env\n' > .git/info/exclude
  printf 'fixture\n' > .env
  printf 'unfinished work\n' > untracked.txt

  git cal </dev/null >/dev/null
  [ -f .env ] && [ -f untracked.txt ] || {
    log_error "git cal deleted files without confirmation"
  }

  printf '1\n' | git cal >/dev/null
  [ ! -e .env ] && [ ! -e untracked.txt ] || {
    log_error "git cal did not clean files after explicit confirmation"
  }
  log_verbose "Cleanup preserves files until explicitly confirmed"
)}

case "$0" in
  *test-git.sh)
    if [ $# -eq 0 ]; then
      set -- config aliases behavior clean
    fi
    for _t in "$@"; do
      "test_${_t}"
    done
    ;;
esac
