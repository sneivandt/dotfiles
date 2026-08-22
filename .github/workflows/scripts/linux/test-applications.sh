#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-applications.sh — Application-level tests for git, zsh, vim, nvim.
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root)
# -----------------------------------------------------------------------------

# shellcheck disable=SC3054
# When sourced with `.`, use BASH_SOURCE if available (bash); otherwise use pwd
if [ -n "${BASH_SOURCE:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
else
  # Fallback: assume we're already in the scripts directory or use relative path
  SCRIPT_DIR="$(pwd)"
fi
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# ---------------------------------------------------------------------------
# Zsh
# ---------------------------------------------------------------------------

test_zsh_completion()
{(
  is_program_installed "zsh" || { log_verbose "Skipping: zsh not installed"; return 0; }
  log_stage "Validating zsh completion"

  completion="$DIR/symlinks/config/zsh/completions/_dotfiles"
  [ -f "$completion" ] || { printf "%sERROR: completion file missing: %s%s\n" "${RED}" "$completion" "${NC}" >&2; return 1; }

  # The generated completion file calls `compdef`, which requires the zsh
  # completion system to be initialised first.
  zsh -c "autoload -Uz compinit; compinit -u; source '$completion'" >/dev/null 2>&1 || { printf "%sERROR: completion failed to load%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Completion file loads OK"

  zsh -c "autoload -Uz compinit; compinit -u; source '$completion' && typeset -f _dotfiles >/dev/null" 2>&1 || { printf "%sERROR: _dotfiles not defined%s\n" "${RED}" "${NC}" >&2; return 1; }
  log_verbose "Completion functions defined"

  # Check profile count matches profiles.toml
  expected=$(list_available_profiles | wc -l)
  loaded=$(zsh -c "
    count=0
    while IFS= read -r l; do [[ \$l =~ '^\[([^]]+)\]\$' ]] && count=\$((count+1)); done < '$DIR/conf/profiles.toml'
    echo \$count
  ")
  [ "$loaded" -eq "$expected" ] || { printf "%sERROR: profile count mismatch: %d vs %d%s\n" "${RED}" "$loaded" "$expected" "${NC}" >&2; return 1; }
  log_verbose "Loaded $loaded profiles"
)}

# ---------------------------------------------------------------------------
# Vim
# ---------------------------------------------------------------------------

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

test_nvim_opens()
{(
  is_program_installed "nvim" || { log_verbose "Skipping: nvim not installed"; return 0; }
  log_stage "Testing nvim startup"

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

# ---------------------------------------------------------------------------
# Volume initialization
# ---------------------------------------------------------------------------

test_volume_init()
{(
  log_stage "Testing volume initialization failures"

  script="$DIR/symlinks/config/volume/init-volume.sh"
  [ -x "$script" ] || { printf "%sERROR: volume initialization script missing or not executable: %s%s\n" "${RED}" "$script" "${NC}" >&2; return 1; }

  fixture="$(mktemp -d)"
  trap 'rm -rf "$fixture"' EXIT
  mock_bin="$fixture/bin"
  output="$fixture/output"
  mkdir -p "$mock_bin"

  if PATH="$mock_bin" "$script" >"$output" 2>&1; then
    printf "%sERROR: volume initialization succeeded without pactl%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  grep -q "pactl is required" "$output" || { printf "%sERROR: missing-pactl failure was not explained%s\n" "${RED}" "${NC}" >&2; return 1; }

  cat > "$mock_bin/pactl" <<'EOF'
#!/bin/sh
case "$1" in
  get-sink-volume)
    [ "${PACTL_SCENARIO:-success}" != "sink-timeout" ] || exit 1
    if [ "${PACTL_SCENARIO:-success}" = "unstable" ]; then
      count=0
      [ ! -f "$PACTL_STATE_FILE" ] || IFS= read -r count < "$PACTL_STATE_FILE"
      count=$((count + 1))
      printf '%s\n' "$count" > "$PACTL_STATE_FILE"
      if [ $((count % 2)) -eq 0 ]; then
        printf 'Volume: 60%%\n'
      else
        printf 'Volume: 70%%\n'
      fi
    else
      printf 'Volume: 70%%\n'
    fi
    ;;
  list)
    printf '1\tmock-sink\n'
    ;;
  set-sink-mute)
    [ "${PACTL_SCENARIO:-success}" != "mutation-failure" ]
    ;;
  set-sink-volume)
    exit 0
    ;;
  *)
    exit 2
    ;;
esac
EOF
  cat > "$mock_bin/sleep" <<'EOF'
#!/bin/sh
exit 0
EOF
  chmod +x "$mock_bin/pactl" "$mock_bin/sleep"
  ln -s "$(command -v awk)" "$mock_bin/awk"

  if PATH="$mock_bin" PACTL_SCENARIO=sink-timeout "$script" >"$output" 2>&1; then
    printf "%sERROR: volume initialization succeeded without a default sink%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  grep -q "no default audio sink" "$output" || { printf "%sERROR: sink-timeout failure was not explained%s\n" "${RED}" "${NC}" >&2; return 1; }

  if PATH="$mock_bin" PACTL_SCENARIO=unstable PACTL_STATE_FILE="$fixture/pactl-state" "$script" >"$output" 2>&1; then
    printf "%sERROR: volume initialization succeeded before volume stabilized%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  grep -q "volume did not stabilize" "$output" || { printf "%sERROR: stabilization timeout was not explained%s\n" "${RED}" "${NC}" >&2; return 1; }

  if PATH="$mock_bin" PACTL_SCENARIO=mutation-failure "$script" >"$output" 2>&1; then
    printf "%sERROR: volume initialization ignored a sink mutation failure%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  PATH="$mock_bin" PACTL_SCENARIO=success "$script" >"$output" 2>&1 || { printf "%sERROR: volume initialization failed with working pactl%s\n" "${RED}" "${NC}" >&2; return 1; }

  cat > "$mock_bin/amixer" <<'EOF'
#!/bin/sh
exit 1
EOF
  chmod +x "$mock_bin/amixer"
  if PATH="$mock_bin" PACTL_SCENARIO=success "$script" >"$output" 2>&1; then
    printf "%sERROR: volume initialization ignored an amixer failure%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  log_verbose "Volume initialization reports missing tools, timeouts, and command failures"
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
