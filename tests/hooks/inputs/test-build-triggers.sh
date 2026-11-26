#!/bin/sh
# shellcheck shell=sh

test_build_version_ref_triggers()
{(
  log_stage "Checking build-version inputs for attached, detached, and linked-worktree HEAD"
  toolchain=$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$DIR/cli/rust-toolchain.toml")
  if ! command -v rustup >/dev/null 2>&1 ||
    ! compiler=$(rustup which --toolchain "$toolchain" rustc 2>/dev/null); then
    log_verbose "Skipping build-version regression: pinned Rust compiler is not installed"
    return 0
  fi
  if command -v cygpath >/dev/null 2>&1; then
    compiler=$(cygpath -u "$compiler")
  fi
  fixture="$DIR/.build-version-inputs-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  # Compile only the std-only build script, without Cargo or dependency builds.
  "$compiler" --edition=2024 "$DIR/cli/build.rs" -o "$fixture/build-script.exe"
  mkdir "$fixture/repo"
  cd "$fixture/repo" || exit 1
  init_fixture_repository
  git symbolic-ref HEAD refs/heads/main
  git -c user.name=Fixture -c user.email=fixture@test.local -c core.hooksPath=/dev/null \
    commit --allow-empty -qm baseline
  main_git_dir=$(git rev-parse --absolute-git-dir)
  unset DOTFILES_VERSION DOTFILES_RELEASE_REPO
  "$fixture/build-script.exe" > "$fixture/no-publisher"
  grep -Fqx 'cargo::rustc-env=DOTFILES_RELEASE_REPO=' "$fixture/no-publisher" ||
    log_error "Source build unexpectedly selected a publisher"
  DOTFILES_RELEASE_REPO=fork-owner/releases "$fixture/build-script.exe" > "$fixture/publisher"
  grep -Fqx 'cargo::rustc-env=DOTFILES_RELEASE_REPO=fork-owner/releases' "$fixture/publisher" ||
    log_error "Build script did not embed the publishing repository"
  grep -Fqx 'cargo::rerun-if-env-changed=DOTFILES_RELEASE_REPO' "$fixture/publisher" ||
    log_error "Publisher changes do not invalidate the build"
  for invalid in 'owner/repo/extra' 'https://github.com/owner/repo' 'owner/repo?query' '../repo'; do
    if DOTFILES_RELEASE_REPO="$invalid" "$fixture/build-script.exe" > "$fixture/invalid" 2>&1; then
      log_error "Build script accepted invalid publisher: $invalid"
    fi
  done
  "$fixture/build-script.exe" > "$fixture/attached"
  grep -Fqx "cargo::rerun-if-changed=$main_git_dir/refs/heads/main" "$fixture/attached" ||
    log_error "Attached HEAD did not watch its branch ref"
  grep -Fqx "cargo::rerun-if-changed=$main_git_dir/HEAD" "$fixture/attached" ||
    log_error "Attached HEAD did not watch branch switches"
  if grep -Fqx "cargo::rerun-if-changed=$main_git_dir/refs/" "$fixture/attached"; then
    log_error "Build version watches unrelated refs"
  fi

  git worktree add -qb fixture-branch "$fixture/worktree"
  (
    unset GIT_DIR GIT_COMMON_DIR GIT_WORK_TREE GIT_INDEX_FILE
    cd "$fixture/worktree" || exit 1
    "$fixture/build-script.exe" > "$fixture/linked"
    worktree_git_dir=$(git rev-parse --absolute-git-dir)
    grep -Fqx "cargo::rerun-if-changed=$worktree_git_dir/HEAD" "$fixture/linked" ||
      log_error "Linked worktree did not watch its own HEAD"
    grep -Fqx "cargo::rerun-if-changed=$main_git_dir/refs/heads/fixture-branch" "$fixture/linked" ||
      log_error "Linked worktree branch was not resolved in the common Git directory"
  )

  git checkout --detach -q
  "$fixture/build-script.exe" > "$fixture/detached"
  grep -Fqx "cargo::rerun-if-changed=$main_git_dir/HEAD" "$fixture/detached" ||
    log_error "Detached HEAD is not watched"
  if grep -q 'cargo::rerun-if-changed=.*refs/heads/' "$fixture/detached"; then
    log_error "Detached HEAD retained an unrelated branch dependency"
  fi
)}
