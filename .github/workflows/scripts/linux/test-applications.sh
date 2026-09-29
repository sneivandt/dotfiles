#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-applications.sh — Application-level tests for git, zsh, vim, nvim.
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root)
# -----------------------------------------------------------------------------

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# ---------------------------------------------------------------------------
# Zsh
# ---------------------------------------------------------------------------

test_zsh_configuration()
{(
  log_stage "Testing isolated shell configuration"
  python3 -B "$SCRIPT_DIR/test-shell-config.py"
)}

test_zsh_completion()
{(
  is_program_installed "zsh" || { log_verbose "Skipping: zsh not installed"; return 0; }
  log_stage "Validating zsh completion"

  completion="$DIR/symlinks/config/zsh/completions/_dotfiles"
  [ -f "$completion" ] || { printf "%sERROR: completion file missing: %s%s\n" "${RED}" "$completion" "${NC}" >&2; return 1; }

  # Runtime completion resolves `dotfiles` when the registration script loads.
  PATH="$DIR/bin:$PATH" zsh -c "autoload -Uz compinit; compinit -u; source '$completion'" >/dev/null 2>&1 || { printf "%sERROR: completion failed to load%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Completion file loads OK"

  PATH="$DIR/bin:$PATH" zsh -c "autoload -Uz compinit; compinit -u; source '$completion' && typeset -f _clap_dynamic_completer_dotfiles >/dev/null" 2>&1 || { printf "%sERROR: runtime dotfiles completer not defined%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Completion functions defined"

)}

test_zsh_history()
{(
  is_program_installed "zsh" || { log_verbose "Skipping: zsh not installed"; return 0; }
  is_program_installed "fzf" || { log_verbose "Skipping: fzf not installed"; return 0; }
  log_stage "Testing zsh history selection"

  fixture="$(mktemp -d)"
  trap 'rm -rf "$fixture"' EXIT
  HOME="$fixture" XDG_CACHE_HOME="$fixture/.cache" DIR="$DIR" \
    zsh -d -f -i -c '
      source "$DIR/symlinks/zshrc"
      [[ "$(bindkey -M emacs "^R")" == "\"^R\" fzf-history-widget" ]] || exit 1
      (( ! $+widgets[fzf-history-widget-accept] )) || exit 1
    '
  log_verbose "Ctrl+R uses the standard non-executing fzf history widget"
)}

# ---------------------------------------------------------------------------
# Vim
# ---------------------------------------------------------------------------

test_vim_configuration()
{(
  log_stage "Testing isolated Vim configuration"
  python3 -B -m unittest discover -s "$DIR/symlinks/vim/tests" -p 'test_*.py'
)}

test_vim_opens()
{(
  is_program_installed "vim" || { log_verbose "Skipping: vim not installed"; return 0; }
  log_stage "Testing vim startup"

  vim --version >/dev/null 2>&1 || { printf "%sERROR: vim --version failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Vim binary OK"

  [ -f "$HOME/.vim/vimrc" ] || { printf "%sERROR: vimrc not installed: %s%s\n" "${RED}" "$HOME/.vim/vimrc" "${NC}" >&2; return 1; }
  timeout 5 vim -E -s -c 'quit' </dev/null >/dev/null 2>&1 || { printf "%sERROR: vim failed to load vimrc%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Vim loads custom vimrc"
)}

# ---------------------------------------------------------------------------
# Neovim
# ---------------------------------------------------------------------------

test_nvim_configuration()
{
  test_vim_configuration
}

test_nvim_opens()
{(
  is_program_installed "nvim" || { log_verbose "Skipping: nvim not installed"; return 0; }
  log_stage "Testing nvim startup"

  DIR="$DIR" timeout 10 nvim --headless -u NONE -i NONE -n \
    -c 'lua local ok, err = pcall(dofile, vim.env.DIR .. "/.github/workflows/scripts/linux/test-nvim-config.lua"); if not ok then vim.api.nvim_err_writeln(tostring(err)); vim.cmd("cquit") end' \
    -c 'qa!' </dev/null

  nvim --version >/dev/null 2>&1 || { printf "%sERROR: nvim --version failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Nvim binary OK"

  [ -d "$HOME/.config/nvim" ] || { printf "%sERROR: nvim config not installed: %s%s\n" "${RED}" "$HOME/.config/nvim" "${NC}" >&2; return 1; }
  timeout 120 nvim --headless -c ':qa!' </dev/null >/dev/null 2>&1 || { printf "%sERROR: nvim failed to load config%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Nvim loads custom config"
)}

test_nvim_plugins()
{(
  is_program_installed "nvim" || { log_verbose "Skipping: nvim not installed"; return 0; }
  [ -f "$HOME/.config/nvim/nvimrc" ] || { printf "%sERROR: nvimrc not installed: %s%s\n" "${RED}" "$HOME/.config/nvim/nvimrc" "${NC}" >&2; return 1; }
  log_stage "Testing nvim plugins"

  lazy_dir="$HOME/.local/share/nvim/lazy"
  timeout 180 nvim --headless '+Lazy! sync' '+qa!' </dev/null >/dev/null 2>&1 || { printf "%sERROR: lazy.nvim failed to synchronize plugins%s\n" "${RED}" "${NC}" >&2; return 1; }
  [ -d "$lazy_dir/lazy.nvim" ] || { printf "%sERROR: lazy.nvim not bootstrapped: %s%s\n" "${RED}" "$lazy_dir/lazy.nvim" "${NC}" >&2; return 1; }

  count=$(find "$lazy_dir" -mindepth 1 -maxdepth 1 -type d | wc -l)
  log_verbose "Found $count plugin directories"

  timeout 30 nvim --headless +'qa!' </dev/null >/dev/null 2>&1 || { printf "%sERROR: nvim plugin load failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Nvim starts with plugins OK"
)}

# ---------------------------------------------------------------------------
# Git
# ---------------------------------------------------------------------------

test_git_config()
{(
  is_program_installed "git" || { log_verbose "Skipping: git not installed"; return 0; }
  log_stage "Testing git configuration"

  git --version >/dev/null 2>&1 || { printf "%sERROR: git --version failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  [ -f "$HOME/.config/git/config" ] || { printf "%sERROR: custom git config not installed: %s%s\n" "${RED}" "$HOME/.config/git/config" "${NC}" >&2; return 1; }
  log_verbose "Custom git config found"

  # Check key config values
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

test_git_aliases()
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

test_git_behavior()
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

  # Default branch should be 'main'
  branch="$(git branch --show-current)"
  if [ "$branch" = "main" ]; then
    log_verbose "✓ Default branch is main"
  else
    printf "%sERROR: default branch is '%s', expected 'main'%s\n" "${RED}" "$branch" "${NC}" >&2
    return 1
  fi

  # Can create a commit
  echo test > test.txt && git add test.txt
  git commit -m "Test commit" >/dev/null 2>&1 || { printf "%sERROR: commit failed%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "✓ Commit created successfully"
)}

test_git_clean()
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

# Execute tests when run directly: sh test-applications.sh <app> <test1> [test2...]
case "$0" in
  *test-applications.sh)
    if [ $# -ge 2 ]; then
      _app="$1"; shift
      for _t in "$@"; do
        "test_${_app}_${_t}"
      done
    fi
    ;;
esac
