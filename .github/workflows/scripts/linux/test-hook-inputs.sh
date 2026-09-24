#!/bin/sh
set -o errexit
set -o nounset

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR/lib/test-helpers.sh"
DIR=${DIR:-$(CDPATH='' cd -- "$SCRIPT_DIR/../../../.." && pwd)}

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

test_fixture_git_context()
{(
  log_stage "Checking fixture isolation from an inherited Git repository and index"
  fixture="$DIR/.ci-git-context-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  mkdir "$fixture/caller" "$fixture/child"
  cd "$fixture/caller"
  init_fixture_repository
  printf 'caller\n' > marker
  git add marker
  caller_tree=$(git write-tree)
  (
    cd "$fixture/child"
    init_fixture_repository
    printf 'child\n' > marker
    git add marker
    [ "$(git show :marker)" = child ] || log_error "Child fixture used the caller index"
  )
  [ "$(git write-tree)" = "$caller_tree" ] || log_error "Child fixture changed the caller index"
)}

test_staged_ci_guards()
{(
  log_stage "Checking CI guards use staged shell, manifest, and config content"
  fixture="$DIR/.ci-guard-inputs-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
  mkdir -p "$fixture/mock-bin" "$fixture/repo [literal]/hooks" "$fixture/repo [literal]/cli" "$fixture/repo [literal]/conf"
  cat > "$fixture/mock-bin/shellcheck" <<'EOF'
#!/bin/sh
set -eu
for file do
  case "$file" in --*) continue ;; esac
  [ "$(cat "$file")" = valid-staged ]
done
printf 'shellcheck\n' >> "$HOOK_INPUT_LOG"
EOF
  cat > "$fixture/mock-bin/cargo" <<'EOF'
#!/bin/sh
set -eu
[ "$1" = run ]
while [ "$#" -gt 0 ]; do
  if [ "$1" = --root ]; then
    shift
    [ "$(cat "$1/conf/fixture.toml")" = staged-config ]
    [ ! -e "$1/conf/untracked.toml" ]
    exit 0
  fi
  shift
done
exit 1
EOF
  chmod +x "$fixture/mock-bin/"*
  PATH="$fixture/mock-bin:$PATH"
  HOOK_INPUT_LOG="$fixture/checks.log"
  export PATH HOOK_INPUT_LOG
  cd "$fixture/repo [literal]"
  init_fixture_repository
  printf 'valid-staged\n' > 'hooks/script with spaces.sh'
  printf '[dependencies]\nexample = "1"\n' > cli/Cargo.toml
  printf 'staged-config\n' > conf/fixture.toml
  git add hooks cli conf
  printf 'invalid-unstaged\n' > 'hooks/script with spaces.sh'
  printf 'unstaged-config\n' > conf/fixture.toml
  printf 'untracked-config\n' > conf/untracked.toml
  sh "$DIR/hooks/check-ci-guards.sh"
  [ "$(cat "$HOOK_INPUT_LOG")" = shellcheck ] || log_error "Staged script was not checked"

  printf 'invalid-staged\n' > 'hooks/script with spaces.sh'
  git add hooks
  printf 'valid-staged\n' > 'hooks/script with spaces.sh'
  if sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
    log_error "CI guards accepted invalid staged shell content"
  fi
  grep -q 'ShellCheck reported issues' "$fixture/output" || log_error "Unexpected staged shell failure"
  git add hooks
  rm 'hooks/script with spaces.sh'
  printf 'staged-config\n' > conf/fixture.toml
  : > "$HOOK_INPUT_LOG"
  sh "$DIR/hooks/check-ci-guards.sh"
  [ "$(cat "$HOOK_INPUT_LOG")" = shellcheck ] || log_error "Unstaged deletion hid a staged script"

  printf 'invalid-staged\n' > conf/fixture.toml
  git add conf/fixture.toml
  printf 'staged-config\n' > conf/fixture.toml
  if sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
    log_error "CI guards accepted invalid staged config hidden by an unstaged fix"
  fi
  grep -q 'configuration validation failed' "$fixture/output" || log_error "Unexpected staged config failure"
  git add conf/fixture.toml

  printf '[dependencies]\nexample = "*"\n' > cli/Cargo.toml
  git add cli
  printf '[dependencies]\nexample = "1"\n' > cli/Cargo.toml
  if sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
    log_error "CI guards accepted a staged wildcard dependency hidden by an unstaged fix"
  fi
  grep -q 'wildcard dependency' "$fixture/output" || log_error "Unexpected staged manifest failure"
)}

test_ci_change_classification()
{(
  log_stage "Checking classification of renames across CI scopes"
  fixture="$DIR/.ci-classification-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
  mkdir -p "$fixture/repo/cli/src" "$fixture/repo/docs"
  cd "$fixture/repo"
  init_fixture_repository
  printf 'fn main() {}\n' > cli/src/main.rs
  git add cli
  BASE_SHA=$(git write-tree)
  git mv cli/src/main.rs docs/example.md
  HEAD_SHA=$(git write-tree)
  GITHUB_OUTPUT="$fixture/outputs"
  export BASE_SHA HEAD_SHA GITHUB_OUTPUT
  DIR="$PWD" GITHUB_EVENT_NAME=pull_request sh "$SCRIPT_DIR/classify-ci-changes.sh"
  grep -qx 'run_rust_checks=true' "$GITHUB_OUTPUT" ||
    log_error "Renaming Rust into docs skipped Rust checks"
  grep -qx 'docs_only=false' "$GITHUB_OUTPUT" ||
    log_error "Renaming Rust into docs was treated as docs-only"

  BASE_SHA=$HEAD_SHA
  printf 'Documentation update\n' >> docs/example.md
  git add docs
  HEAD_SHA=$(git write-tree)
  : > "$GITHUB_OUTPUT"
  DIR="$PWD" GITHUB_EVENT_NAME=pull_request sh "$SCRIPT_DIR/classify-ci-changes.sh"
  grep -qx 'docs_only=true' "$GITHUB_OUTPUT" || log_error "Docs-only changes lost their fast path"
  grep -qx 'run_rust_checks=false' "$GITHUB_OUTPUT" || log_error "Docs-only changes ran Rust checks"
  grep -qx 'run_lint=false' "$GITHUB_OUTPUT" || log_error "Docs-only changes enabled managed-script checks through lint"
  grep -qx 'run_build_artifacts=false' "$GITHUB_OUTPUT" || log_error "Docs-only changes enabled managed-script checks through builds"

  for path in \
    symlinks/config/powershell/tests/Test-Prompt.ps1 \
    symlinks/config/hypr/scripts/tests/test_lock_screen.py \
    symlinks/config/quickshell/tests/python/test_power_menu.py; do
    BASE_SHA=$HEAD_SHA
    mkdir -p "$(dirname "$path")"
    printf 'managed-script fixture\n' > "$path"
    git add -- "$path"
    HEAD_SHA=$(git write-tree)
    : > "$GITHUB_OUTPUT"
    DIR="$PWD" GITHUB_EVENT_NAME=pull_request sh "$SCRIPT_DIR/classify-ci-changes.sh"
    grep -qx 'docs_only=false' "$GITHUB_OUTPUT" || log_error "$path was classified as documentation"
    grep -qx 'run_lint=true' "$GITHUB_OUTPUT" || log_error "$path did not enable managed-script checks through lint"
    grep -qx 'run_build_artifacts=true' "$GITHUB_OUTPUT" || log_error "$path did not enable managed-script checks through builds"
  done
)}

test_staged_ci_guard_deletions()
{(
  log_stage "Checking full CI guards classify staged deletions and both rename paths"
  fixture="$DIR/.ci-guard-deletions-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  mkdir -p "$fixture/mock-bin" "$fixture/repo/cli" "$fixture/repo/conf" "$fixture/repo/symlinks" "$fixture/repo/docs" "$fixture/repo/hooks"
  cat > "$fixture/mock-bin/cargo" <<'EOF'
#!/bin/sh
set -eu
[ "$1" = run ]
printf 'config\n' >> "$HOOK_INPUT_LOG"
while [ "$#" -gt 0 ]; do
  if [ "$1" = --root ]; then
    shift
    [ -f "$1/conf/required.toml" ] && [ -f "$1/symlinks/required" ]
    exit $?
  fi
  shift
done
exit 1
EOF
  cat > "$fixture/mock-bin/shellcheck" <<'EOF'
#!/bin/sh
echo "Deleted-only scripts must not be sent to ShellCheck" >&2
exit 1
EOF
  chmod +x "$fixture/mock-bin/"*
  PATH="$fixture/mock-bin:$PATH"
  HOOK_INPUT_LOG="$fixture/checks.log"
  DOTFILES_HOOKS_FULL=1
  export PATH HOOK_INPUT_LOG DOTFILES_HOOKS_FULL
  cd "$fixture/repo"
  init_fixture_repository
  printf '[package]\nname = "fixture"\nversion = "0.1.0"\n' > cli/Cargo.toml
  printf 'required\n' > conf/required.toml
  printf 'required\n' > symlinks/required
  printf '#!/bin/sh\nexit 0\n' > hooks/removed.sh
  git add cli conf symlinks hooks
  git -c user.name=Fixture -c user.email=fixture@test.local -c core.hooksPath=/dev/null \
    commit -qm baseline

  for change in delete-config rename-config rename-source; do
    git reset --hard -q HEAD
    mkdir -p docs
    case "$change" in
      delete-config)
        git rm -q conf/required.toml
        # An unstaged restoration must not conceal the staged deletion.
        mkdir -p conf
        printf 'required\n' > conf/required.toml
        ;;
      rename-config) git mv conf/required.toml docs/required.md ;;
      rename-source) git mv symlinks/required docs/required.txt ;;
    esac
    : > "$HOOK_INPUT_LOG"
    if sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
      log_error "Full guards accepted $change"
    fi
    [ "$(cat "$HOOK_INPUT_LOG")" = config ] || log_error "$change did not run config validation"
    grep -q 'configuration validation failed' "$fixture/output" ||
      log_error "$change failed for an unexpected reason"
  done

  git reset --hard -q HEAD
  git rm -q hooks/removed.sh
  sh "$DIR/hooks/check-ci-guards.sh"
)}

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
  cd "$fixture/repo"
  init_fixture_repository
  git symbolic-ref HEAD refs/heads/main
  git -c user.name=Fixture -c user.email=fixture@test.local -c core.hooksPath=/dev/null \
    commit --allow-empty -qm baseline
  main_git_dir=$(git rev-parse --absolute-git-dir)
  unset DOTFILES_VERSION
  "$fixture/build-script.exe" > "$fixture/attached"
  grep -Fqx "cargo:rerun-if-changed=$main_git_dir/refs/heads/main" "$fixture/attached" ||
    log_error "Attached HEAD did not watch its branch ref"
  grep -Fqx "cargo:rerun-if-changed=$main_git_dir/HEAD" "$fixture/attached" ||
    log_error "Attached HEAD did not watch branch switches"
  if grep -Fqx "cargo:rerun-if-changed=$main_git_dir/refs/" "$fixture/attached"; then
    log_error "Build version watches unrelated refs"
  fi

  git worktree add -qb fixture-branch "$fixture/worktree"
  (
    unset GIT_DIR GIT_COMMON_DIR GIT_WORK_TREE GIT_INDEX_FILE
    cd "$fixture/worktree"
    "$fixture/build-script.exe" > "$fixture/linked"
    worktree_git_dir=$(git rev-parse --absolute-git-dir)
    grep -Fqx "cargo:rerun-if-changed=$worktree_git_dir/HEAD" "$fixture/linked" ||
      log_error "Linked worktree did not watch its own HEAD"
    grep -Fqx "cargo:rerun-if-changed=$main_git_dir/refs/heads/fixture-branch" "$fixture/linked" ||
      log_error "Linked worktree branch was not resolved in the common Git directory"
  )

  git checkout --detach -q
  "$fixture/build-script.exe" > "$fixture/detached"
  grep -Fqx "cargo:rerun-if-changed=$main_git_dir/HEAD" "$fixture/detached" ||
    log_error "Detached HEAD is not watched"
  if grep -q 'cargo:rerun-if-changed=.*refs/heads/' "$fixture/detached"; then
    log_error "Detached HEAD retained an unrelated branch dependency"
  fi
)}

if [ "$#" -gt 0 ]; then
  for test_name do
    "$test_name"
  done
  exit 0
fi

fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null
mkdir -p "$fixture/mock-bin" "$fixture/index repo/cli/src" "$fixture/index repo/conf"

cat > "$fixture/mock-bin/cargo" <<'EOF'
#!/bin/sh
set -eu
printf '%s\n' "$1" >> "$HOOK_INPUT_LOG"
[ "$(cat src/lib.rs)" = valid-staged ]
[ "$(cat ../conf/fixture.toml)" = staged-config ]
[ ! -e src/untracked.rs ]
EOF
cat > "$fixture/mock-bin/rustc" <<'EOF'
#!/bin/sh
exit 1
EOF
cat > "$fixture/mock-bin/pwsh" <<'EOF'
#!/bin/sh
set -eu
file=${PS_FILES%;}
[ "$(cat "$file")" = valid-staged ]
printf 'powershell\n' >> "$HOOK_INPUT_LOG"
EOF
cat > "$fixture/mock-bin/shellcheck" <<'EOF'
#!/bin/sh
set -eu
for file do
  case "$file" in --*) continue ;; esac
  [ -f "$file" ] || { printf 'Missing ShellCheck input: %s\n' "$file" >&2; exit 2; }
  basename "$file" >> "$HOOK_INPUT_LOG"
done
exit "${SHELLCHECK_EXIT:-0}"
EOF
chmod +x "$fixture/mock-bin/"*
PATH="$fixture/mock-bin:$PATH"
export PATH
HOOK_INPUT_LOG="$fixture/checks.log"
export HOOK_INPUT_LOG

log_stage "Checking index-only Rust and PowerShell hook inputs"
(
  cd "$fixture/index repo"
  init_fixture_repository
  printf '[package]\nname = "fixture"\nversion = "0.1.0"\n' > cli/Cargo.toml
  printf 'invalid-staged\n' > cli/src/lib.rs
  printf 'valid-staged\n' > 'script with spaces.ps1'
  printf 'staged-config\n' > conf/fixture.toml
  git add cli conf 'script with spaces.ps1'
  printf 'valid-staged\n' > cli/src/lib.rs
  if sh "$DIR/hooks/check-rust.sh" > "$fixture/output" 2>&1; then
    log_error "Rust hook accepted invalid staged content hidden by an unstaged fix"
  fi
  grep -q 'Commit aborted' "$fixture/output" || log_error "Rust hook failure was not explained"

  git add cli/src/lib.rs
  printf 'invalid-unstaged\n' > cli/src/lib.rs
  printf 'unstaged-config\n' > conf/fixture.toml
  printf 'untracked\n' > cli/src/untracked.rs
  rm 'script with spaces.ps1'
  : > "$HOOK_INPUT_LOG"
  DOTFILES_HOOKS_FULL=1 sh "$DIR/hooks/check-rust.sh"
  expected=$(printf '%s\n' fmt clippy test powershell)
  [ "$(cat "$HOOK_INPUT_LOG")" = "$expected" ] || log_error "Not all checks ran against staged content"

  printf 'invalid-staged\n' > 'script with spaces.ps1'
  git add 'script with spaces.ps1'
  printf 'valid-staged\n' > 'script with spaces.ps1'
  if sh "$DIR/hooks/check-rust.sh" > "$fixture/output" 2>&1; then
    log_error "PowerShell hook accepted invalid staged content hidden by an unstaged fix"
  fi
  grep -q 'PSScriptAnalyzer reported issues' "$fixture/output" || log_error "PowerShell hook failure was not explained"
)

log_stage "Checking ShellCheck paths containing spaces and glob characters"
mkdir -p "$fixture/shell repo [literal]/hooks" "$fixture/shell repo [literal]/symlinks"
(
  cd "$fixture/shell repo [literal]"
  init_fixture_repository
  printf '#!/bin/sh\nexit 0\n' > 'hooks/one file.sh'
  printf '#!/bin/sh\nexit 0\n' > 'hooks/glob[script].sh'
  cp 'hooks/one file.sh' dotfiles.sh
  cp 'hooks/one file.sh' 'symlinks/another file.sh'
  git add hooks
  : > "$HOOK_INPUT_LOG"
  sh "$DIR/hooks/check-ci-guards.sh"
  [ "$(sort "$HOOK_INPUT_LOG")" = "$(printf '%s\n' 'glob[script].sh' 'one file.sh')" ] ||
    log_error "Hook ShellCheck arguments did not preserve complete filenames"

  : > "$HOOK_INPUT_LOG"
  DIR="$PWD" sh "$SCRIPT_DIR/test-static-analysis.sh" test_shellcheck
  expected=$(printf '%s\n' 'another file.sh' dotfiles.sh 'glob[script].sh' 'one file.sh')
  [ "$(sort "$HOOK_INPUT_LOG")" = "$expected" ] ||
    log_error "Static-analysis ShellCheck arguments did not preserve complete filenames"

  if SHELLCHECK_EXIT=1 sh "$DIR/hooks/check-ci-guards.sh" > "$fixture/output" 2>&1; then
    log_error "Hook ignored a ShellCheck failure"
  fi
  if DIR="$PWD" SHELLCHECK_EXIT=1 sh "$SCRIPT_DIR/test-static-analysis.sh" test_shellcheck > "$fixture/output" 2>&1; then
    log_error "Static analysis ignored a ShellCheck failure"
  fi
)
log_verbose "Hook input regression tests passed"
test_fixture_git_context
test_staged_ci_guards
test_ci_change_classification
test_staged_ci_guard_deletions
test_build_version_ref_triggers
