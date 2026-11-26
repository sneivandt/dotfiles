#!/bin/sh
# shellcheck shell=sh

init_fixture_repository() {
  fixture_repo=$(pwd)
  fixture_git_root="$fixture_repo"
  if command -v cygpath >/dev/null 2>&1; then
    fixture_git_root=$(cygpath -m "$fixture_repo")
  fi
  # Hooks can inherit the caller's Git context; pin every subprocess to this
  # fixture before issuing any Git command, including init and configuration.
  unset GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG GIT_CONFIG_PARAMETERS
  GIT_DIR="$fixture_git_root/.git"
  GIT_COMMON_DIR="$GIT_DIR"
  GIT_WORK_TREE="$fixture_git_root"
  GIT_INDEX_FILE="$GIT_DIR/index"
  GIT_CONFIG_NOSYSTEM=1
  GIT_CONFIG_COUNT=0
  GIT_CONFIG_GLOBAL="$fixture_git_root/.empty-gitconfig"
  : > "$GIT_CONFIG_GLOBAL"
  export GIT_DIR GIT_COMMON_DIR GIT_WORK_TREE GIT_INDEX_FILE
  export GIT_CONFIG_NOSYSTEM GIT_CONFIG_COUNT GIT_CONFIG_GLOBAL
  git init -q --template= "$fixture_repo"
  actual_root=$(CDPATH='' cd -- "$(git rev-parse --show-toplevel)" && pwd)
  [ "$actual_root" = "$fixture_repo" ] ||
    log_error "Git escaped the fixture repository: expected '$fixture_repo', got '$actual_root'"
  git config core.autocrlf false
}

