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
OWN_CHECK_STAGE_DIR=""
CHECK_STAGE_DIR=""
CHECK_ROOT=""
trap '[ -z "$OWN_CHECK_STAGE_DIR" ] || rm -rf "$OWN_CHECK_STAGE_DIR"' EXIT
trap 'exit 1' HUP INT TERM

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
      'packages/*/PKGBUILD' 'packages/*/*.install' > "$CHECK_STAGE_DIR/.shell-inputs"
    if [ ! -s "$CHECK_STAGE_DIR/.shell-inputs" ]; then
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
    ' sh "$analyzer" < "$CHECK_STAGE_DIR/.shell-inputs"); then
      abort_with_hint \
        "ShellCheck reported issues." \
        "sh cli/src/app/validation/scripts/shellcheck.sh <staged shell files>"
    fi
  else
    printf '%sSkipping ShellCheck: shellcheck not installed.%s\n' "$DIM" "$NC"
  fi
}

run_powershell_guards() {
  if command -v pwsh >/dev/null 2>&1; then
    prepare_staged_tree
    printf "Running PSScriptAnalyzer...\n"
    # Keep NUL delimiters through argv construction, including tabs/newlines.
    # Prefix with ./ so a tracked filename cannot become an analyzer option.
    git diff --cached --name-only --diff-filter=d -z "$against" -- \
      '*.ps1' '*.psm1' '*.psd1' > "$CHECK_STAGE_DIR/.powershell-inputs"
    if [ ! -s "$CHECK_STAGE_DIR/.powershell-inputs" ]; then
      return 0
    fi
    analyzer="$(CDPATH='' cd -- "$(dirname -- "$0")/../cli/src/app/validation/scripts" && pwd)/psscriptanalyzer.ps1"
    if ! (cd "$CHECK_ROOT" && xargs -0 sh -c '
      analyzer=$1
      shift
      for file do
        set -- "$@" "./$file"
        shift
      done
      exec pwsh -NoProfile -File "$analyzer" "$@"
    ' sh "$analyzer" < "$CHECK_STAGE_DIR/.powershell-inputs") 2>&1; then
      abort_with_hint \
        "PSScriptAnalyzer reported issues." \
        "pwsh -NoProfile -File cli/src/app/validation/scripts/psscriptanalyzer.ps1 <staged PowerShell files>"
    fi
  else
    printf '%sSkipping PSScriptAnalyzer: pwsh not installed.%s\n' "$DIM" "$NC"
  fi
}

run_python_guards() {
  if command -v ruff >/dev/null 2>&1; then
    prepare_staged_tree
    printf "Running Ruff...\n"
    git diff --cached --name-only --no-renames --diff-filter=d -z "$against" -- \
      '*.py' symlinks/local/bin/code-insiders > "$CHECK_STAGE_DIR/.python-inputs"
    if [ ! -s "$CHECK_STAGE_DIR/.python-inputs" ]; then
      return 0
    fi
    ruff_config=""
    if [ -f "$REPO_ROOT/.github/linters/ruff.toml" ]; then
      ruff_config="$REPO_ROOT/.github/linters/ruff.toml"
    elif [ -f "$WORKTREE_ROOT/.github/linters/ruff.toml" ]; then
      ruff_config="$WORKTREE_ROOT/.github/linters/ruff.toml"
    fi
    if ! (cd "$CHECK_ROOT" && xargs -0 sh -c '
      config=$1
      shift
      for file do
        case "$file" in
          -*) file="./$file" ;;
        esac
        set -- "$@" "$file"
        shift
      done
      if [ -n "$config" ]; then
        exec ruff check --config "$config" -- "$@"
      fi
      exec ruff check -- "$@"
    ' sh "$ruff_config" < "$CHECK_STAGE_DIR/.python-inputs"); then
      abort_with_hint \
        "Ruff reported Python lint issues." \
        "ruff check --config .github/linters/ruff.toml <staged python files>"
    fi
  else
    printf '%sSkipping Ruff: ruff not installed.%s\n' "$DIM" "$NC"
  fi
}

run_lua_guards() {
  if command -v luacheck >/dev/null 2>&1; then
    prepare_staged_tree
    printf "Running Luacheck...\n"
    git diff --cached --name-only --no-renames --diff-filter=d -z "$against" -- \
      '*.lua' > "$CHECK_STAGE_DIR/.lua-inputs"
    if [ ! -s "$CHECK_STAGE_DIR/.lua-inputs" ]; then
      return 0
    fi
    luacheck_config=""
    if [ -f "$REPO_ROOT/.github/linters/luacheckrc" ]; then
      luacheck_config="$REPO_ROOT/.github/linters/luacheckrc"
    elif [ -f "$WORKTREE_ROOT/.github/linters/luacheckrc" ]; then
      luacheck_config="$WORKTREE_ROOT/.github/linters/luacheckrc"
    fi
    if ! (cd "$CHECK_ROOT" && xargs -0 sh -c '
      config=$1
      shift
      for file do
        case "$file" in
          -*) file="./$file" ;;
        esac
        set -- "$@" "$file"
        shift
      done
      if [ -n "$config" ]; then
        exec luacheck --config "$config" -- "$@"
      fi
      exec luacheck -- "$@"
    ' sh "$luacheck_config" < "$CHECK_STAGE_DIR/.lua-inputs"); then
      abort_with_hint \
        "Luacheck reported Lua lint issues." \
        "luacheck --config .github/linters/luacheckrc <staged lua files>"
    fi
  else
    printf '%sSkipping Luacheck: luacheck not installed.%s\n' "$DIM" "$NC"
  fi
}

run_workflow_lint_guards() {
  if command -v actionlint >/dev/null 2>&1; then
    prepare_staged_tree
    git diff --cached --name-only --no-renames --diff-filter=d -z "$against" -- \
      '.github/workflows/*.yml' '.github/workflows/*.yaml' > "$CHECK_STAGE_DIR/.workflow-inputs"
    if [ ! -s "$CHECK_STAGE_DIR/.workflow-inputs" ]; then
      return 0
    fi
    printf "Running actionlint...\n"
    if ! (cd "$CHECK_ROOT" && xargs -0 sh -c '
      for file do
        case "$file" in
          -*) file="./$file" ;;
        esac
        set -- "$@" "$file"
        shift
      done
      exec actionlint "$@"
    ' sh < "$CHECK_STAGE_DIR/.workflow-inputs"); then
      abort_with_hint \
        "actionlint reported GitHub Actions workflow issues." \
        "actionlint .github/workflows/*.yml"
    fi
  else
    printf '%sSkipping actionlint: actionlint not installed.%s\n' "$DIM" "$NC"
  fi
}

run_ci_contract_guards() {
  if ! command -v python3 >/dev/null 2>&1; then
    printf '%sSkipping CI contract guards: python3 not installed.%s\n' "$DIM" "$NC"
    return
  fi

  prepare_staged_tree
  contract="$REPO_ROOT/.github/workflows/scripts/linux/check-ci-contract.py"
  if [ ! -f "$REPO_ROOT/.github/workflows/ci.yml" ] || [ ! -f "$contract" ]; then
    return 0
  fi
  printf "Running CI contract guards...\n"
  if ! python3 -B "$contract" 2>&1; then
    abort_with_hint \
      "CI workflow contract check failed." \
      "python3 -B .github/workflows/scripts/linux/check-ci-contract.py"
  fi
}

run_doc_links_guards() {
  if ! command -v python3 >/dev/null 2>&1; then
    printf '%sSkipping doc link guards: python3 not installed.%s\n' "$DIM" "$NC"
    return
  fi

  prepare_staged_tree
  checker="$REPO_ROOT/.github/workflows/scripts/linux/check-doc-links.py"
  if [ ! -f "$checker" ]; then
    return 0
  fi
  printf "Running documentation link checks...\n"
  if ! python3 -B "$checker" "$REPO_ROOT" 2>&1; then
    abort_with_hint \
      "documentation link check failed." \
      "python3 -B .github/workflows/scripts/linux/check-doc-links.py ."
  fi
}

run_skills_guards() {
  if ! command -v python3 >/dev/null 2>&1; then
    printf '%sSkipping skill metadata guards: python3 not installed.%s\n' "$DIM" "$NC"
    return
  fi

  prepare_staged_tree
  checker="$REPO_ROOT/.agents/skills/skill-maintenance/scripts/check-skills.py"
  if [ ! -f "$checker" ]; then
    return 0
  fi
  printf "Running agent skill metadata checks...\n"
  if ! python3 -B "$checker" "$REPO_ROOT" 2>&1; then
    abort_with_hint \
      "agent skill metadata check failed." \
      "python3 -B .agents/skills/skill-maintenance/scripts/check-skills.py ."
  fi
}

run_wrapper_parity_guards() {
  if ! command -v python3 >/dev/null 2>&1; then
    printf '%sSkipping wrapper parity guards: python3 not installed.%s\n' "$DIM" "$NC"
    return
  fi

  prepare_staged_tree
  checker="$REPO_ROOT/tests/wrapper/test-parity.py"
  if [ ! -f "$checker" ]; then
    return 0
  fi
  printf "Running wrapper parity checks...\n"
  if ! python3 -B "$checker" 2>&1; then
    abort_with_hint \
      "wrapper parity check failed." \
      "python3 -B tests/wrapper/test-parity.py"
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

if has_staged_match '*.ps1' '*.psm1' '*.psd1'; then
  run_powershell_guards
fi

if has_staged_match '*.py' symlinks/local/bin/code-insiders; then
  run_python_guards
fi

if has_staged_match '*.lua'; then
  run_lua_guards
fi

if has_staged_match dotfiles.sh dotfiles.ps1 'tests/wrapper/*'; then
  run_wrapper_parity_guards
fi

if full_checks_enabled && has_staged_match dotfiles.sh tests/wrapper/test-shell-wrapper.sh 'tests/wrapper/linux/*' tests/wrapper/parity-contract.json tests/wrapper/test-parity.py; then
  run_wrapper_guards
elif has_staged_match dotfiles.sh tests/wrapper/test-shell-wrapper.sh 'tests/wrapper/linux/*' tests/wrapper/parity-contract.json tests/wrapper/test-parity.py; then
  printf '%sSkipping Linux shell wrapper tests: set DOTFILES_HOOKS_FULL=1 to run them.%s\n' "$DIM" "$NC"
fi

if has_staged_match '.github/workflows/*.yml' '.github/workflows/*.yaml'; then
  run_workflow_lint_guards
fi

if has_staged_match .github/workflows/release.yml; then
  run_release_workflow_guards
fi

if has_staged_match \
  .github/workflows/ci.yml \
  .github/workflows/pages.yml \
  .github/workflows/mutation.yml \
  .github/workflows/dependency-security.yml \
  .github/workflows/scripts/linux/ci_metadata.py \
  .github/workflows/scripts/linux/workflow_yaml.py \
  .github/workflows/scripts/linux/classify-ci-changes.py \
  .github/workflows/scripts/linux/check-ci-contract.py; then
  run_ci_contract_guards
fi

if has_staged_match '*.md' .github/workflows/scripts/linux/check-doc-links.py; then
  run_doc_links_guards
fi

if has_staged_match '.agents/skills/*' AGENTS.md; then
  run_skills_guards
fi

exit 0
