#!/bin/sh
# shellcheck disable=SC3043,SC2154  # 'local' is widely supported; color variables sourced from logger.sh
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
    printf "%sERROR: Completion file failed to load%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  log_verbose "Completion file loads successfully"

  # Test that main functions are defined
  if ! zsh -c "source '$completion_file' && typeset -f _dotfiles >/dev/null" 2>&1; then
    printf "%sERROR: _dotfiles function not defined%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  if ! zsh -c "source '$completion_file' && typeset -f _dotfiles_get_profiles >/dev/null" 2>&1; then
    printf "%sERROR: _dotfiles_get_profiles function not defined%s\n" "${RED}" "${NC}" >&2
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
    printf "%sERROR: No profiles loaded from profiles.ini%s\n" "${RED}" "${NC}" >&2
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
    printf "%sERROR: -I does not exclude --install in its exclusion list%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  # Extract the line with --install definition and check it excludes -I
  if ! grep -- ")--install\[" "$completion_file" | grep -qF -- "-I"; then
    printf "%sERROR: --install does not exclude -I in its exclusion list%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  # Check that all command flags exclude help (both -h and --help)
  # Test a few key flags with their actual patterns in the file
  for pattern in "'-I'\[" ")--install\[" "'-T'\[" ")--test\[" "'-U'\[" ")--uninstall\["; do
    # Find the line defining this flag and check it has -h and --help in exclusions
    local flag_line
    flag_line=$(grep -- "$pattern" "$completion_file" || true)

    if [ -z "$flag_line" ]; then
      printf "%sERROR: Could not find definition for pattern %s%s\n" "${RED}" "${pattern}" "${NC}" >&2
      return 1
    fi

    if ! echo "$flag_line" | grep -qF -- " -h " || \
       ! echo "$flag_line" | grep -qF -- " --help"; then
      printf "%sERROR: Flag matching %s does not exclude both help flags%s\n" "${RED}" "${pattern}" "${NC}" >&2
    fi
  done

  # Check that help excludes everything with (- *)
  if ! grep -F -- "(- *)'-h'" "$completion_file" >/dev/null || \
     ! grep -F -- "(- *)--help[" "$completion_file" >/dev/null; then
    printf "%sERROR: Help flags do not properly exclude all other options%s\n" "${RED}" "${NC}" >&2
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
    printf "%sERROR: Cannot run vim --version%s\n" "${RED}" "${NC}" >&2
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
        printf "%sERROR: Vim failed to load custom vimrc%s\n" "${RED}" "${NC}" >&2
        return 1
      fi
    else
      # Fallback without timeout
      if vim -E -s -c 'quit' </dev/null >/dev/null 2>&1; then
        log_verbose "Vim loads custom vimrc successfully"
      else
        printf "%sERROR: Vim failed to load custom vimrc%s\n" "${RED}" "${NC}" >&2
        return 1
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
    printf "%sERROR: Cannot run nvim --version%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  log_verbose "Nvim binary is functional"

  # Test 2: Check if nvim config directory exists (supports various config layouts)
  if [ -d "$HOME/.config/nvim" ]; then
    log_verbose "Custom nvim config directory found"

    # Test that nvim can start with the custom config in headless mode
    # Note: First run may take longer due to lazy.nvim bootstrap (git clone)
    if is_program_installed "timeout"; then
      if timeout 30 nvim --headless -c ':qa!' </dev/null >/dev/null 2>&1; then
        log_verbose "Nvim loads custom config successfully"
      else
        printf "%sERROR: Nvim failed to load custom config%s\n" "${RED}" "${NC}" >&2
        return 1
      fi
    else
      # Fallback without timeout
      if nvim --headless -c ':qa!' </dev/null >/dev/null 2>&1; then
        log_verbose "Nvim loads custom config successfully"
      else
        printf "%sERROR: Nvim failed to load custom config%s\n" "${RED}" "${NC}" >&2
        return 1
      fi
    fi
  else
    log_verbose "No custom nvim config installed, basic nvim test complete"
  fi
)}

# test_nvim_plugins
#
# Test that Neovim plugins are installed and can be loaded.
# Validates lazy.nvim plugin manager installation and all configured plugins.
test_nvim_plugins()
{(
  # Check if nvim is installed
  if ! is_program_installed "nvim"; then
    log_verbose "Skipping nvim plugin test: nvim not installed"
    return 0
  fi

  # Check if lazy-bootstrap.lua exists (may be excluded by sparse checkout)
  local lazy_config="$DIR/symlinks/vim/lua/lazy-bootstrap.lua"
  if [ ! -f "$lazy_config" ]; then
    log_verbose "Skipping nvim plugin test: lazy-bootstrap.lua not found"
    return 0
  fi

  # Check if nvim config is installed
  if [ ! -f "$HOME/.config/nvim/nvimrc" ]; then
    log_verbose "Skipping nvim plugin test: nvimrc not installed"
    return 0
  fi

  log_stage "Testing nvim plugin installation"

  local errors_file
  errors_file="$(mktemp)"
  echo 0 > "$errors_file"

  # Set timeout command
  local timeout_cmd="timeout 30"
  if ! is_program_installed "timeout"; then
    timeout_cmd=""
    log_verbose "timeout not available, tests may hang if nvim blocks"
  fi

  # Test 1: Check if lazy.nvim directory exists
  local lazy_path="$HOME/.local/share/nvim/lazy/lazy.nvim"
  if [ ! -d "$lazy_path" ]; then
    printf "%sWARNING: lazy.nvim not installed at expected path%s\n" "${YELLOW}" "${NC}" >&2
    printf "%sRun :Lazy sync in nvim to install plugins%s\n" "${YELLOW}" "${NC}" >&2
    rm -f "$errors_file"
    return 0
  else
    log_verbose "lazy.nvim is installed at $lazy_path"
  fi

  # Test 2: Count installed plugins by checking directories
  log_verbose "Checking installed plugin directories"
  local plugin_dir="$HOME/.local/share/nvim/lazy"
  local plugin_count=0

  if [ -d "$plugin_dir" ]; then
    # Count directories in lazy plugin directory
    plugin_count=$(find "$plugin_dir" -mindepth 1 -maxdepth 1 -type d | wc -l)
    log_verbose "Found $plugin_count plugin directories in $plugin_dir"

    if [ "$plugin_count" -lt 5 ]; then
      printf "%sWARNING: Only %d plugins found (expected 20+)%s\n" "${YELLOW}" "$plugin_count" "${NC}" >&2
      printf "%sRun :Lazy sync in nvim to install missing plugins%s\n" "${YELLOW}" "${NC}" >&2
    fi
  else
    printf "%sERROR: Plugin directory not found: %s%s\n" "${RED}" "$plugin_dir" "${NC}" >&2
    errors=$(cat "$errors_file")
    echo $((errors + 1)) > "$errors_file"
  fi

  # Test 3: Verify specific critical plugins exist
  log_verbose "Checking critical plugin directories"

  local critical_plugins="lazy.nvim fzf.vim nvim-tree.lua lualine.nvim tokyonight.nvim vim-fugitive"
  local missing_count=0

  for plugin in $critical_plugins; do
    if [ ! -d "$plugin_dir/$plugin" ]; then
      log_verbose "Missing critical plugin: $plugin"
      missing_count=$((missing_count + 1))
    fi
  done

  if [ "$missing_count" -gt 0 ]; then
    printf "%sWARNING: %d critical plugin(s) not installed%s\n" "${YELLOW}" "$missing_count" "${NC}" >&2
    printf "%sRun :Lazy sync in nvim to install missing plugins%s\n" "${YELLOW}" "${NC}" >&2
  else
    log_verbose "All critical plugins are installed"
  fi

  # Test 4: Try loading nvim with configuration in headless mode
  log_verbose "Testing nvim can load with plugin configuration"

  local load_test
  # Use minimal test - just start and quit
  load_test="$($timeout_cmd nvim --headless +'qa!' 2>&1 >/dev/null || echo "FAILED")"

  if [ "$load_test" = "FAILED" ] || [ -n "$load_test" ]; then
    printf "%sERROR: nvim failed to start with plugin configuration%s\n" "${RED}" "${NC}" >&2
    if [ -n "$load_test" ]; then
      printf "%sError output: %s%s\n" "${RED}" "$load_test" "${NC}" >&2
    fi
    errors=$(cat "$errors_file")
    echo $((errors + 1)) > "$errors_file"
  else
    log_verbose "nvim starts successfully with plugin configuration"
  fi

  # Test 5: Verify lazy-lock.json exists (indicates plugins were successfully installed)
  local lock_file="$DIR/symlinks/vim/lazy-lock.json"
  if [ ! -f "$lock_file" ]; then
    printf "%sWARNING: lazy-lock.json not found (plugins may not be locked)%s\n" "${YELLOW}" "${NC}" >&2
  else
    log_verbose "lazy-lock.json found - plugin versions are locked"

    # Count plugins in lockfile
    local locked_plugins
    locked_plugins="$(grep -c '": {' "$lock_file" || echo "0")"
    log_verbose "lazy-lock.json contains $locked_plugins plugin entries"

    if [ "$locked_plugins" -lt 15 ]; then
      printf "%sWARNING: Only %d plugins in lazy-lock.json (expected 20+)%s\n" "${YELLOW}" "$locked_plugins" "${NC}" >&2
    fi
  fi

  # Check for errors
  errors=$(cat "$errors_file")
  rm -f "$errors_file"

  if [ "$errors" -gt 0 ]; then
    printf "${RED}Nvim plugin validation found %d error(s)${NC}\n" "$errors" >&2
    return 1
  else
    log_verbose "Nvim plugin validation passed"
  fi
)}

# test_git_config
#
# Test that git configuration is properly loaded.
# Validates that custom git config files are installed and accessible.
test_git_config()
{(
  # Check if git is installed
  if ! is_program_installed "git"; then
    log_verbose "Skipping git config test: git not installed"
    return 0
  fi

  log_stage "Testing git configuration"

  # Test 1: Verify git can run
  if ! git --version >/dev/null 2>&1; then
    printf "%sERROR: Cannot run git --version%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  log_verbose "Git binary is functional"

  # Test 2: Check if custom git config is installed
  if [ ! -f "$HOME/.config/git/config" ]; then
    log_verbose "Custom git config not installed yet"
    return 0
  fi

  log_verbose "Custom git config found at ~/.config/git/config"

  # Test 3: Verify config file includes the custom config
  # Git should be reading from ~/.config/git/config due to XDG_CONFIG_HOME
  local test_config
  test_config="$(git config --get core.pager 2>/dev/null || echo "")"

  if [ -z "$test_config" ]; then
    printf "%sWARNING: core.pager not set (git config may not be loaded)%s\n" "${YELLOW}" "${NC}" >&2
  else
    log_verbose "Git config is loaded: core.pager = $test_config"
  fi

  # Test 4: Verify key configuration values from the custom config
  log_verbose "Checking key configuration values"

  # Check init.defaultBranch
  local default_branch
  default_branch="$(git config --get init.defaultBranch 2>/dev/null || echo "")"
  if [ "$default_branch" = "main" ]; then
    log_verbose "✓ init.defaultBranch is set to 'main'"
  else
    printf "%sWARNING: init.defaultBranch is not set to 'main' (got: %s)%s\n" "${YELLOW}" "${default_branch}" "${NC}" >&2
  fi

  # Check pull.rebase
  local pull_rebase
  pull_rebase="$(git config --get pull.rebase 2>/dev/null || echo "")"
  if [ "$pull_rebase" = "true" ]; then
    log_verbose "✓ pull.rebase is enabled"
  else
    printf "%sWARNING: pull.rebase is not enabled (got: %s)%s\n" "${YELLOW}" "${pull_rebase}" "${NC}" >&2
  fi

  # Check merge.conflictstyle
  local conflict_style
  conflict_style="$(git config --get merge.conflictstyle 2>/dev/null || echo "")"
  if [ "$conflict_style" = "zdiff3" ]; then
    log_verbose "✓ merge.conflictstyle is set to 'zdiff3'"
  else
    printf "%sWARNING: merge.conflictstyle is not set to 'zdiff3' (got: %s)%s\n" "${YELLOW}" "${conflict_style}" "${NC}" >&2
  fi

  # Check push.autoSetupRemote
  local auto_setup
  auto_setup="$(git config --get push.autoSetupRemote 2>/dev/null || echo "")"
  if [ "$auto_setup" = "true" ]; then
    log_verbose "✓ push.autoSetupRemote is enabled"
  else
    printf "%sWARNING: push.autoSetupRemote is not enabled (got: %s)%s\n" "${YELLOW}" "${auto_setup}" "${NC}" >&2
  fi

  # Check diff.algorithm
  local diff_algo
  diff_algo="$(git config --get diff.algorithm 2>/dev/null || echo "")"
  if [ "$diff_algo" = "histogram" ]; then
    log_verbose "✓ diff.algorithm is set to 'histogram'"
  else
    printf "%sWARNING: diff.algorithm is not set to 'histogram' (got: %s)%s\n" "${YELLOW}" "${diff_algo}" "${NC}" >&2
  fi

  log_verbose "Git configuration test passed"
)}

# test_git_aliases
#
# Test that git aliases are defined and functional.
# Validates that custom aliases from the config are loaded.
test_git_aliases()
{(
  # Check if git is installed
  if ! is_program_installed "git"; then
    log_verbose "Skipping git aliases test: git not installed"
    return 0
  fi

  log_stage "Testing git aliases"

  # Check if custom git config is installed
  if [ ! -f "$HOME/.config/git/config" ]; then
    log_verbose "Custom git config not installed yet"
    return 0
  fi

  # Check if aliases file exists
  if [ ! -f "$HOME/.config/git/aliases" ]; then
    log_verbose "Git aliases file not installed yet"
    return 0
  fi

  log_verbose "Git aliases file found at ~/.config/git/aliases"

  # Test a few key aliases to ensure they're loaded
  local test_aliases="st br lo ci"
  local missing_count=0

  for alias_name in $test_aliases; do
    if git config --get "alias.$alias_name" >/dev/null 2>&1; then
      local alias_value
      alias_value="$(git config --get "alias.$alias_name")"
      log_verbose "✓ alias.$alias_name = $alias_value"
    else
      printf "%sWARNING: alias.%s is not defined%s\n" "${YELLOW}" "$alias_name" "${NC}" >&2
      missing_count=$((missing_count + 1))
    fi
  done

  if [ "$missing_count" -gt 0 ]; then
    printf "%sWARNING: %d aliases are missing%s\n" "${YELLOW}" "$missing_count" "${NC}" >&2
  fi

  # Test that 'git alias' command works (lists all aliases)
  if git config --get "alias.alias" >/dev/null 2>&1; then
    log_verbose "✓ alias.alias command is defined"

    # Try running the alias command (should list all aliases)
    if git alias 2>/dev/null | grep -q "^alias"; then
      log_verbose "✓ 'git alias' command executes successfully"
    else
      printf "%sWARNING: 'git alias' command did not produce expected output%s\n" "${YELLOW}" "${NC}" >&2
    fi
  else
    printf "%sWARNING: alias.alias command is not defined%s\n" "${YELLOW}" "${NC}" >&2
  fi

  log_verbose "Git aliases test passed"
)}

# test_git_behavior
#
# Test that git behavior settings work correctly.
# Creates a temporary repository and tests actual git operations.
test_git_behavior()
{(
  # Check if git is installed
  if ! is_program_installed "git"; then
    log_verbose "Skipping git behavior test: git not installed"
    return 0
  fi

  log_stage "Testing git behavior"

  # Check if custom git config is installed
  if [ ! -f "$HOME/.config/git/config" ]; then
    log_verbose "Custom git config not installed yet"
    return 0
  fi

  # Create a temporary test repository
  local test_repo
  test_repo="$(mktemp -d)"
  log_verbose "Creating test repository at $test_repo"

  cd "$test_repo"

  # Initialize repository and configure user
  if ! git init >/dev/null 2>&1; then
    printf "%sERROR: Failed to initialize test repository%s\n" "${RED}" "${NC}" >&2
    cd - >/dev/null
    rm -rf "$test_repo"
    return 1
  fi

  git config user.name "Test User"
  git config user.email "test@example.com"

  log_verbose "Test repository initialized"

  # Test 1: Verify default branch is 'main'
  local current_branch
  current_branch="$(git branch --show-current)"
  if [ "$current_branch" = "main" ]; then
    log_verbose "✓ Default branch is 'main'"
  else
    printf "%sWARNING: Default branch is '%s', expected 'main'%s\n" "${YELLOW}" "$current_branch" "${NC}" >&2
  fi

  # Test 2: Test ignore patterns
  if [ -f "$HOME/.config/git/ignore" ]; then
    log_verbose "Testing global ignore patterns"

    # Create a file that should be ignored
    touch node_modules
    mkdir -p test_dir
    touch test_dir/.DS_Store

    # Check if files are ignored
    # Note: Global ignore only works when core.excludesfile is set with full path
    local excludesfile
    excludesfile="$(git config --get core.excludesfile 2>/dev/null || echo "")"

    if [ -n "$excludesfile" ]; then
      log_verbose "core.excludesfile is configured: $excludesfile"

      # Test if pattern actually works (node_modules should be ignored)
      if git status --porcelain 2>/dev/null | grep -q "node_modules"; then
        log_verbose "Note: Global ignore patterns may need absolute path in config"
      else
        log_verbose "✓ Global ignore patterns are working"
      fi
    else
      log_verbose "Note: core.excludesfile not configured"
    fi
  else
    log_verbose "Global ignore file not installed"
  fi

  # Test 3: Test attributes
  if [ -f "$HOME/.config/git/attributes" ]; then
    log_verbose "Testing global attributes"

    # Create a test file to check attributes
    echo "test content" > test.py

    # Check if text attribute is applied (from attributes file: * text=auto)
    if git check-attr text test.py 2>/dev/null | grep -q "text: auto"; then
      log_verbose "✓ Global attributes are working (text=auto)"
    else
      log_verbose "Note: Global attributes may not show in this test"
    fi
  else
    log_verbose "Global attributes file not installed"
  fi

  # Test 4: Create a commit and test config
  echo "test" > test.txt
  git add test.txt

  if git commit -m "Test commit" >/dev/null 2>&1; then
    log_verbose "✓ Successfully created test commit"

    # Verify commit exists
    if git log --oneline 2>/dev/null | grep -q "Test commit"; then
      log_verbose "✓ Commit is in history"
    fi
  else
    printf "%sWARNING: Failed to create test commit%s\n" "${YELLOW}" "${NC}" >&2
  fi

  # Clean up
  cd - >/dev/null
  rm -rf "$test_repo"

  log_verbose "Git behavior test passed"
)}
