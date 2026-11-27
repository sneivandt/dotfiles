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

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=hooks/staged-tree.sh
. "$SCRIPT_DIR/staged-tree.sh"

init_staged_tree_context

resolve_repo_file() {
  rel_path="$1"
  if [ -n "$CHECK_ROOT" ] && [ -f "$CHECK_ROOT/$rel_path" ]; then
    printf '%s\n' "$CHECK_ROOT/$rel_path"
  elif [ -f "$SCRIPT_DIR/../$rel_path" ]; then
    printf '%s\n' "$SCRIPT_DIR/../$rel_path"
  else
    return 1
  fi
}

find_ci_metadata() {
  if [ -n "$CHECK_ROOT" ] && [ -f "$CHECK_ROOT/.github/workflows/scripts/linux/ci_metadata.py" ]; then
    printf '%s\n' "$CHECK_ROOT/.github/workflows/scripts/linux/ci_metadata.py"
  elif [ -f "$WORKTREE_ROOT/.github/workflows/scripts/linux/ci_metadata.py" ]; then
    printf '%s\n' "$WORKTREE_ROOT/.github/workflows/scripts/linux/ci_metadata.py"
  elif [ -f "$SCRIPT_DIR/../.github/workflows/scripts/linux/ci_metadata.py" ]; then
    printf '%s\n' "$SCRIPT_DIR/../.github/workflows/scripts/linux/ci_metadata.py"
  else
    return 1
  fi
}

run_cargo() {
  (
    cd "$REPO_ROOT/cli" &&
      CARGO_TARGET_DIR="$HOOK_CARGO_TARGET_DIR" \
      DOTFILES_VERSION="${DOTFILES_VERSION:-dev-hook}" \
      cargo "$@"
  )
}

has_staged_match() {
  # Used only to detect staged changes to ci_metadata.py itself before
  # exporting the staged tree.
  changed=$(git diff --cached --name-only --no-renames "$against" -- "$@") || exit 1
  [ -n "$changed" ]
}

CI_METADATA=""

require_ci_metadata() {
  if ! command -v python3 >/dev/null 2>&1; then
    printf '%sSkipping CI guards: python3 not installed.%s\n' "$DIM" "$NC"
    exit 0
  fi
  if ! CI_METADATA=$(find_ci_metadata); then
    printf '%sSkipping CI guards: ci_metadata.py not found.%s\n' "$DIM" "$NC"
    exit 0
  fi
}

has_staged_category() {
  subcmd="$1"
  category="$2"
  tmp_list=$(mktemp)
  if ! git diff --cached --name-only --no-renames -z "$against" -- > "$tmp_list"; then
    rm -f "$tmp_list"
    exit 1
  fi
  status=0
  python3 -B "$CI_METADATA" "$subcmd" "$category" < "$tmp_list" || status=$?
  rm -f "$tmp_list"
  if [ "$status" -eq 0 ]; then
    return 0
  fi
  [ "$status" -eq 1 ] || exit "$status"
  return 1
}

has_staged_linter() {
  has_staged_category has-linter "$1"
}

has_staged_guard() {
  has_staged_category has-guard "$1"
}

staged_linter_inputs() {
  category="$1"
  output="$2"
  all_staged="$CHECK_STAGE_DIR/.all-staged"
  if [ ! -f "$all_staged" ]; then
    git diff --cached --name-only --no-renames --diff-filter=d -z "$against" -- > "$all_staged"
  fi
  python3 -B "$CI_METADATA" filter-linter "$category" < "$all_staged" > "$output"
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
    toolchain_check=$(resolve_repo_file "hooks/check-toolchain.sh")
    sh "$toolchain_check" "$REPO_ROOT/cli"
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
    staged_linter_inputs shell "$CHECK_STAGE_DIR/.shell-inputs"
    if [ ! -s "$CHECK_STAGE_DIR/.shell-inputs" ]; then
      return 0
    fi
    analyzer=$(resolve_repo_file "cli/src/app/validation/scripts/shellcheck.sh")
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
    staged_linter_inputs powershell "$CHECK_STAGE_DIR/.powershell-inputs"
    if [ ! -s "$CHECK_STAGE_DIR/.powershell-inputs" ]; then
      return 0
    fi
    analyzer=$(resolve_repo_file "cli/src/app/validation/scripts/psscriptanalyzer.ps1")
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
    staged_linter_inputs python "$CHECK_STAGE_DIR/.python-inputs"
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
        exec ruff format --check --config "$config" -- "$@"
      fi
      exec ruff format --check -- "$@"
    ' sh "$ruff_config" < "$CHECK_STAGE_DIR/.python-inputs"); then
      abort_with_hint \
        "Ruff reported Python formatting issues." \
        "ruff format --config .github/linters/ruff.toml <staged python files>"
    fi
  else
    printf '%sSkipping Ruff: ruff not installed.%s\n' "$DIM" "$NC"
  fi
}

run_lua_guards() {
  if command -v luacheck >/dev/null 2>&1; then
    prepare_staged_tree
    printf "Running Luacheck...\n"
    staged_linter_inputs lua "$CHECK_STAGE_DIR/.lua-inputs"
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

run_structured_guards() {
  prepare_staged_tree
  checker=$(resolve_repo_file ".github/workflows/scripts/linux/check-structured-files.py") || return 0
  staged_linter_inputs structured "$CHECK_STAGE_DIR/.structured-inputs"
  if [ ! -s "$CHECK_STAGE_DIR/.structured-inputs" ]; then
    return 0
  fi
  printf "Running structured file checks...\n"
  if ! (cd "$CHECK_ROOT" && xargs -0 sh -c '
    checker=$1
    shift
    for file do
      case "$file" in
        -*) file="./$file" ;;
      esac
      set -- "$@" "$file"
      shift
    done
    exec python3 -B "$checker" "$@"
  ' sh "$checker" < "$CHECK_STAGE_DIR/.structured-inputs"); then
    abort_with_hint \
      "Structured file validation reported issues." \
      "python3 -B .github/workflows/scripts/linux/check-structured-files.py --root ."
  fi
}

run_workflow_lint_guards() {
  if command -v actionlint >/dev/null 2>&1; then
    prepare_staged_tree
    staged_linter_inputs workflows "$CHECK_STAGE_DIR/.workflow-inputs"
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

  prepare_staged_tree
  printf "Running release workflow guards...\n"

  if ! workflow_contents=$(git show ":.github/workflows/release.yml"); then
    abort_with_hint \
      "staged release workflow cannot be read." \
      "stage .github/workflows/release.yml before running the publishing guards"
  fi

  contract=$(resolve_repo_file ".github/workflows/scripts/linux/check-ci-contract.py")
  if ! printf '%s\n' "$workflow_contents" | python3 -B "$contract" --release - 2>&1; then
    abort_with_hint \
      "release workflow contract check failed." \
      "python3 -B .github/workflows/scripts/linux/check-ci-contract.py"
  fi
}

if has_staged_match .github/workflows/scripts/linux/ci_metadata.py; then
  prepare_staged_tree
fi

require_ci_metadata

if has_staged_guard config; then
  run_config_validation
fi

if has_staged_guard dependency; then
  run_dependency_guards
fi

if has_staged_linter shell; then
  run_shell_guards
fi

if has_staged_linter powershell; then
  run_powershell_guards
fi

if has_staged_linter python; then
  run_python_guards
fi

if has_staged_linter lua; then
  run_lua_guards
fi

if has_staged_linter structured; then
  run_structured_guards
fi

if has_staged_guard wrapper-parity; then
  run_wrapper_parity_guards
fi

if has_staged_guard wrapper-linux; then
  if full_checks_enabled; then
    run_wrapper_guards
  else
    printf '%sSkipping Linux shell wrapper tests: set DOTFILES_HOOKS_FULL=1 to run them.%s\n' "$DIM" "$NC"
  fi
fi

if has_staged_linter workflows; then
  run_workflow_lint_guards
fi

if has_staged_guard release-workflow; then
  run_release_workflow_guards
fi

if has_staged_guard ci-contract; then
  run_ci_contract_guards
fi

if has_staged_guard doc-links; then
  run_doc_links_guards
fi

if has_staged_guard skills; then
  run_skills_guards
fi

exit 0
