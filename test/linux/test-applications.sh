#!/bin/sh
# shellcheck disable=SC3043  # 'local' is widely supported even if not strictly POSIX
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-applications.sh
# -----------------------------------------------------------------------------
# Application-level tests for installed tools (vim, nvim, zsh).
#
# Functions:
#   test_zsh_completion  Validate zsh completion file structure
#   test_vim_opens       Test that vim can start and exit without errors
#   test_nvim_opens      Test that neovim can start and exit without errors
#
# Dependencies:
#   logger.sh (log_stage, log_verbose)
#   utils.sh  (is_program_installed, list_available_profiles)
#
# Expected Environment Variables:
#   DIR  Repository root directory (exported by dotfiles.sh)
# -----------------------------------------------------------------------------

# DIR is exported by dotfiles.sh
# shellcheck disable=SC2154

. "$DIR"/src/linux/logger.sh
. "$DIR"/src/linux/utils.sh

# test_zsh_completion
#
# Validate zsh completion file structure and profile loading.
# Checks that completion can be loaded and dynamically reads profiles.
test_zsh_completion()
{(
  # Skip if zsh is not installed
  if ! is_program_installed "zsh"; then
    log_verbose "Skipping zsh completion test: zsh not installed"
    return 0
  fi

  log_stage "Validating zsh completion"

  local completion_file="$DIR/symlinks/config/zsh/completions/_dotfiles"

  # Check that completion file exists
  if [ ! -f "$completion_file" ]; then
    printf "${RED}ERROR: Completion file not found: %s${NC}\n" "$completion_file" >&2
    return 1
  fi

  log_verbose "Testing completion file structure"

  # Test that completion loads without errors
  if ! zsh -c "source '$completion_file' 2>&1" >/dev/null 2>&1; then
    printf "${RED}ERROR: Completion file failed to load${NC}\n" >&2
    return 1
  fi

  log_verbose "Completion file loads successfully"

  # Test that main functions are defined
  if ! zsh -c "source '$completion_file' && typeset -f _dotfiles >/dev/null" 2>&1; then
    printf "${RED}ERROR: _dotfiles function not defined${NC}\n" >&2
    return 1
  fi

  if ! zsh -c "source '$completion_file' && typeset -f _dotfiles_get_profiles >/dev/null" 2>&1; then
    printf "${RED}ERROR: _dotfiles_get_profiles function not defined${NC}\n" >&2
    return 1
  fi

  log_verbose "Completion functions are defined"

  # Test profile loading from profiles.ini
  # We can't call _dotfiles_get_profiles directly as it uses _describe which only
  # works in completion context, so we test the logic by checking file reading
  log_verbose "Testing profile path resolution and file reading"

  profiles_loaded=$(zsh -c "
    cd '$DIR'
    profile_file='$DIR/conf/profiles.ini'
    count=0
    if [[ -f \"\$profile_file\" ]]; then
      while IFS= read -r line; do
        if [[ \$line =~ '^\[([^]]+)\]\$' ]]; then
          count=\$((count + 1))
        fi
      done < \"\$profile_file\"
    fi
    echo \$count
  ")

  if [ -z "$profiles_loaded" ] || [ "$profiles_loaded" -eq 0 ]; then
    printf "${RED}ERROR: No profiles loaded from profiles.ini${NC}\n" >&2
    return 1
  fi

  log_verbose "Loaded $profiles_loaded profile(s) from profiles.ini"

  # Verify loaded profiles match those in profiles.ini
  expected_profiles=$(list_available_profiles | wc -l)
  if [ "$profiles_loaded" -ne "$expected_profiles" ]; then
    printf "${RED}ERROR: Profile count mismatch: expected %d, got %d${NC}\n" "$expected_profiles" "$profiles_loaded" >&2
    return 1
  fi

  # Validate mutual exclusivity patterns in completion
  log_verbose "Validating mutual exclusivity patterns"

  # Extract the line with -I definition and check it excludes --install
  if ! grep -- "'-I'\[" "$completion_file" | grep -qF -- "--install"; then
    printf "${RED}ERROR: -I does not exclude --install in its exclusion list${NC}\n" >&2
    return 1
  fi

  # Extract the line with --install definition and check it excludes -I
  if ! grep -- ")--install\[" "$completion_file" | grep -qF -- "-I"; then
    printf "${RED}ERROR: --install does not exclude -I in its exclusion list${NC}\n" >&2
    return 1
  fi

  # Check that all command flags exclude help (both -h and --help)
  # Test a few key flags with their actual patterns in the file
  for pattern in "'-I'\[" ")--install\[" "'-T'\[" ")--test\[" "'-U'\[" ")--uninstall\["; do
    # Find the line defining this flag and check it has -h and --help in exclusions
    local flag_line
    flag_line=$(grep -- "$pattern" "$completion_file" || true)

    if [ -z "$flag_line" ]; then
      printf "${RED}ERROR: Could not find definition for pattern ${pattern}${NC}\n" >&2
      return 1
    fi

    if ! echo "$flag_line" | grep -qF -- " -h " || \
       ! echo "$flag_line" | grep -qF -- " --help"; then
      printf "${RED}ERROR: Flag matching ${pattern} does not exclude both help flags${NC}\n" >&2
    fi
  done

  # Check that help excludes everything with (- *)
  if ! grep -F -- "(- *)'-h'" "$completion_file" >/dev/null || \
     ! grep -F -- "(- *)--help[" "$completion_file" >/dev/null; then
    printf "${RED}ERROR: Help flags do not properly exclude all other options${NC}\n" >&2
    return 1
  fi

  log_verbose "Mutual exclusivity patterns validated"

  log_verbose "Zsh completion validation passed"
)}

# test_vim_opens
#
# Test that vim can start and exit without errors.
# This is a basic smoke test to ensure vim is functional.
test_vim_opens()
{(
  # Check if vim is installed
  if ! is_program_installed "vim"; then
    log_verbose "Skipping vim test: vim not installed"
    return 0
  fi

  log_stage "Testing vim startup"

  # Test 1: Check vim version (ensures binary works)
  if ! vim --version >/dev/null 2>&1; then
    printf "${RED}ERROR: Cannot run vim --version${NC}\n" >&2
    return 1
  fi

  log_verbose "Vim binary is functional"

  # Test 2: Check if custom vimrc is installed
  if [ -f "$HOME/.vim/vimrc" ]; then
    log_verbose "Custom vimrc found at ~/.vim/vimrc"

    # Test that vim can start with the custom vimrc
    # Use ex mode with immediate quit and explicit stdin redirect
    if is_program_installed "timeout"; then
      if timeout 5 vim -E -s -c 'quit' </dev/null >/dev/null 2>&1; then
        log_verbose "Vim loads custom vimrc successfully"
      else
        printf "${YELLOW}WARNING: Vim may have issues loading custom vimrc${NC}\n" >&2
      fi
    else
      # Fallback without timeout
      if vim -E -s -c 'quit' </dev/null >/dev/null 2>&1; then
        log_verbose "Vim loads custom vimrc successfully"
      else
        printf "${YELLOW}WARNING: Vim may have issues loading custom vimrc${NC}\n" >&2
      fi
    fi
  else
    log_verbose "No custom vimrc installed, basic vim test complete"
  fi
)}

# test_nvim_opens
#
# Test that neovim can start and exit without errors.
# This is a basic smoke test to ensure nvim is functional.
test_nvim_opens()
{(
  # Check if nvim is installed
  if ! is_program_installed "nvim"; then
    log_verbose "Skipping nvim test: nvim not installed"
    return 0
  fi

  log_stage "Testing nvim startup"

  # Test 1: Check nvim version (ensures binary works)
  if ! nvim --version >/dev/null 2>&1; then
    printf "${RED}ERROR: Cannot run nvim --version${NC}\n" >&2
    return 1
  fi

  log_verbose "Nvim binary is functional"

  # Test 2: Check if nvim config directory exists (supports various config layouts)
  if [ -d "$HOME/.config/nvim" ]; then
    log_verbose "Custom nvim config directory found"

    # Test that nvim can start with the custom config in headless mode
    if is_program_installed "timeout"; then
      if timeout 5 nvim --headless -c ':qa!' </dev/null >/dev/null 2>&1; then
        log_verbose "Nvim loads custom config successfully"
      else
        printf "${YELLOW}WARNING: Nvim may have issues loading custom config${NC}\n" >&2
      fi
    else
      # Fallback without timeout
      if nvim --headless -c ':qa!' </dev/null >/dev/null 2>&1; then
        log_verbose "Nvim loads custom config successfully"
      else
        printf "${YELLOW}WARNING: Nvim may have issues loading custom config${NC}\n" >&2
      fi
    fi
  else
    log_verbose "No custom nvim config installed, basic nvim test complete"
  fi
)}
