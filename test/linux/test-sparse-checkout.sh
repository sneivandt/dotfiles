#!/bin/sh
# shellcheck disable=SC3043,SC2154,SC2030,SC2031  # 'local' is widely supported; variables sourced from logger.sh/utils.sh; subshell modifications intentional for test isolation
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-sparse-checkout.sh
# -----------------------------------------------------------------------------
# Tests for configure_sparse_checkout function in utils.sh
#
# Functions:
#   test_sparse_checkout_profile_not_found
#   test_sparse_checkout_profile_with_exclusions
#   test_sparse_checkout_profile_with_includes
#   test_sparse_checkout_auto_exclude_arch_on_non_arch
#   test_sparse_checkout_auto_exclude_windows_on_linux
#   test_sparse_checkout_skip_os_detection_flag
#   test_sparse_checkout_os_detection_combinations
#   test_sparse_checkout_pattern_generation
#   test_sparse_checkout_unchanged_config_skipped
#   test_sparse_checkout_config_change_detected
#   test_sparse_checkout_manifest_interaction
#   test_sparse_checkout_not_git_repository
#   test_sparse_checkout_uncommitted_changes_docker
#   test_sparse_checkout_git_reset_sequence
#   test_sparse_checkout_dry_run_no_modifications
#   test_sparse_checkout_dry_run_validates_config
#   test_sparse_checkout_idempotent
#
# Dependencies:
#   logger.sh (log_stage, log_error, log_verbose)
#   utils.sh  (configure_sparse_checkout, parse_profile)
#
# Expected Environment Variables:
#   DIR  Repository root directory (exported by dotfiles.sh)
# -----------------------------------------------------------------------------

# DIR is exported by dotfiles.sh
# shellcheck disable=SC2154

. "$DIR"/src/linux/logger.sh
. "$DIR"/src/linux/utils.sh

# _create_test_repo
#
# Helper function to create a temporary git repository with test structure
#
# Args:
#   $1  repository directory path
_create_test_repo()
{
  local repo_dir="$1"

  # Create repo structure
  mkdir -p "$repo_dir"
  cd "$repo_dir"

  # Initialize git repo
  git init -q
  git config user.name "Test User"
  git config user.email "test@example.com"

  # Create basic directory structure
  mkdir -p conf
  mkdir -p symlinks/config
  mkdir -p src/linux

  # Create initial files
  echo "test" > symlinks/config/test.txt
  echo "base" > symlinks/base.txt
  echo "arch" > symlinks/arch.txt
  echo "windows" > symlinks/windows.txt
  echo "desktop" > symlinks/desktop.txt

  # Commit initial state
  git add -A
  git commit -q -m "Initial commit"
}

# _create_test_profiles_ini
#
# Helper function to create a test profiles.ini file
#
# Args:
#   $1  target directory
_create_test_profiles_ini()
{
  local target_dir="$1"

  cat > "$target_dir/conf/profiles.ini" <<'EOF'
[base]
include=
exclude=windows,arch,desktop

[arch]
include=arch
exclude=windows,desktop

[arch-desktop]
include=arch,desktop
exclude=windows

[windows]
include=windows,desktop
exclude=arch

[desktop]
include=desktop
exclude=windows,arch
EOF
}

# _create_test_manifest_ini
#
# Helper function to create a test manifest.ini file
#
# Args:
#   $1  target directory
_create_test_manifest_ini()
{
  local target_dir="$1"

  cat > "$target_dir/conf/manifest.ini" <<'EOF'
[windows]
windows.txt
config/windows

[arch]
arch.txt
config/arch

[desktop]
desktop.txt
config/desktop

[arch,desktop]
config/arch-desktop
EOF
}

# test_sparse_checkout_profile_not_found
#
# Test that configure_sparse_checkout fails with error when profile doesn't exist
test_sparse_checkout_profile_not_found()
{(
  log_stage "Testing profile not found error handling"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original DIR
  local original_dir="$DIR"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"

  # Set DIR to test repo
  DIR="$test_repo"

  # Test with non-existent profile (should fail)
  # Run in subshell to prevent exit from killing the test
  local exit_code=0
  (configure_sparse_checkout "nonexistent-profile") 2>/dev/null || exit_code=$?

  if [ "$exit_code" -eq 0 ]; then
    DIR="$original_dir"
    log_error "Expected error for non-existent profile, but succeeded"
  fi

  # Restore DIR
  DIR="$original_dir"

  log_verbose "Profile not found error handling passed"
)}

# test_sparse_checkout_profile_with_exclusions
#
# Test that configure_sparse_checkout correctly parses profile with exclusions
test_sparse_checkout_profile_with_exclusions()
{(
  log_stage "Testing profile with exclusions"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR and OPT
  DIR="$test_repo"
  OPT=" --skip-os-detection "
  export EXCLUDED_CATEGORIES=""

  # Test with base profile (excludes windows,arch,desktop)
  configure_sparse_checkout "base"

  # Verify EXCLUDED_CATEGORIES is set correctly
  case ",$EXCLUDED_CATEGORIES," in
    *,windows,*) ;;
    *) DIR="$original_dir"; OPT="$original_opt"; log_error "Expected 'windows' in EXCLUDED_CATEGORIES" ;;
  esac

  case ",$EXCLUDED_CATEGORIES," in
    *,arch,*) ;;
    *) DIR="$original_dir"; OPT="$original_opt"; log_error "Expected 'arch' in EXCLUDED_CATEGORIES" ;;
  esac

  case ",$EXCLUDED_CATEGORIES," in
    *,desktop,*) ;;
    *) DIR="$original_dir"; OPT="$original_opt"; log_error "Expected 'desktop' in EXCLUDED_CATEGORIES" ;;
  esac

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Profile with exclusions passed"
)}

# test_sparse_checkout_profile_with_includes
#
# Test that configure_sparse_checkout correctly parses profile with includes
test_sparse_checkout_profile_with_includes()
{(
  log_stage "Testing profile with includes"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR and OPT
  DIR="$test_repo"
  OPT=" --skip-os-detection "
  export EXCLUDED_CATEGORIES=""

  # Test with arch profile (includes arch, excludes windows,desktop)
  configure_sparse_checkout "arch"

  # Verify arch is NOT excluded
  case ",$EXCLUDED_CATEGORIES," in
    *,arch,*)
      DIR="$original_dir"; OPT="$original_opt"
      log_error "Did not expect 'arch' in EXCLUDED_CATEGORIES for arch profile"
      ;;
  esac

  # Verify windows and desktop ARE excluded
  case ",$EXCLUDED_CATEGORIES," in
    *,windows,*) ;;
    *) DIR="$original_dir"; OPT="$original_opt"; log_error "Expected 'windows' in EXCLUDED_CATEGORIES" ;;
  esac

  case ",$EXCLUDED_CATEGORIES," in
    *,desktop,*) ;;
    *) DIR="$original_dir"; OPT="$original_opt"; log_error "Expected 'desktop' in EXCLUDED_CATEGORIES" ;;
  esac

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Profile with includes passed"
)}

# test_sparse_checkout_auto_exclude_arch_on_non_arch
#
# Test that non-Arch systems automatically exclude 'arch' category
test_sparse_checkout_auto_exclude_arch_on_non_arch()
{(
  log_stage "Testing auto-exclude arch on non-Arch system"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"
  local original_is_arch="$IS_ARCH"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR and mock IS_ARCH=0 (non-Arch)
  DIR="$test_repo"
  OPT=""  # No skip-os-detection flag
  IS_ARCH=0
  export EXCLUDED_CATEGORIES=""

  # Test with arch-desktop profile that includes arch
  configure_sparse_checkout "arch-desktop"

  # Verify arch was auto-excluded
  case ",$EXCLUDED_CATEGORIES," in
    *,arch,*) ;;
    *) DIR="$original_dir"; OPT="$original_opt"; IS_ARCH="$original_is_arch"; log_error "Expected 'arch' to be auto-excluded on non-Arch system" ;;
  esac

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"
  IS_ARCH="$original_is_arch"

  log_verbose "Auto-exclude arch on non-Arch system passed"
)}

# test_sparse_checkout_auto_exclude_windows_on_linux
#
# Test that Linux systems automatically exclude 'windows' category
test_sparse_checkout_auto_exclude_windows_on_linux()
{(
  log_stage "Testing auto-exclude windows on Linux system"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR
  DIR="$test_repo"
  OPT=""  # No skip-os-detection flag
  export EXCLUDED_CATEGORIES=""

  # Test with windows profile that includes windows
  configure_sparse_checkout "windows"

  # Verify windows was auto-excluded (always on Linux)
  case ",$EXCLUDED_CATEGORIES," in
    *,windows,*) ;;
    *) DIR="$original_dir"; OPT="$original_opt"; log_error "Expected 'windows' to be auto-excluded on Linux system" ;;
  esac

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Auto-exclude windows on Linux system passed"
)}

# test_sparse_checkout_skip_os_detection_flag
#
# Test that --skip-os-detection flag bypasses auto-detection
test_sparse_checkout_skip_os_detection_flag()
{(
  log_stage "Testing --skip-os-detection flag"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"
  local original_is_arch="$IS_ARCH"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"

  # Set DIR with skip-os-detection flag and mock IS_ARCH=0 (non-Arch)
  DIR="$test_repo"
  OPT=" --skip-os-detection "
  IS_ARCH=0
  export EXCLUDED_CATEGORIES=""

  # Test with arch-desktop profile (includes arch, excludes windows)
  configure_sparse_checkout "arch-desktop"

  # With --skip-os-detection, arch should NOT be auto-excluded even on non-Arch
  case ",$EXCLUDED_CATEGORIES," in
    *,arch,*)
      DIR="$original_dir"; OPT="$original_opt"; IS_ARCH="$original_is_arch"
      log_error "Did not expect 'arch' to be auto-excluded with --skip-os-detection flag"
      ;;
  esac

  # But windows should still be excluded (it's in the profile)
  case ",$EXCLUDED_CATEGORIES," in
    *,windows,*) ;;
    *) DIR="$original_dir"; OPT="$original_opt"; IS_ARCH="$original_is_arch"; log_error "Expected 'windows' to be excluded (from profile)" ;;
  esac

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"
  IS_ARCH="$original_is_arch"

  log_verbose "--skip-os-detection flag passed"
)}

# test_sparse_checkout_os_detection_combinations
#
# Test combinations of profile exclusions and OS detection
test_sparse_checkout_os_detection_combinations()
{(
  log_stage "Testing OS detection combinations"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"
  local original_is_arch="$IS_ARCH"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR and mock IS_ARCH=0 (non-Arch)
  DIR="$test_repo"
  OPT=""  # No skip-os-detection flag
  IS_ARCH=0
  export EXCLUDED_CATEGORIES=""

  # Test with desktop profile (excludes windows,arch already)
  # Should not duplicate arch exclusion
  configure_sparse_checkout "desktop"

  # Count occurrences of 'arch' in EXCLUDED_CATEGORIES
  local arch_count
  arch_count="$(echo ",$EXCLUDED_CATEGORIES," | grep -o ",arch," | wc -l)"

  if [ "$arch_count" -ne 1 ]; then
    DIR="$original_dir"; OPT="$original_opt"; IS_ARCH="$original_is_arch"
    log_error "Expected 'arch' to appear once in EXCLUDED_CATEGORIES, found $arch_count times"
  fi

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"
  IS_ARCH="$original_is_arch"

  log_verbose "OS detection combinations passed"
)}

# test_sparse_checkout_pattern_generation
#
# Test that sparse checkout patterns are generated correctly
test_sparse_checkout_pattern_generation()
{(
  log_stage "Testing sparse checkout pattern generation"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR and OPT
  DIR="$test_repo"
  OPT=" --skip-os-detection "
  export EXCLUDED_CATEGORIES=""

  # Configure sparse checkout with arch profile
  configure_sparse_checkout "arch"

  # Check that sparse-checkout list contains expected patterns
  local sparse_config
  sparse_config="$(git -C "$test_repo" sparse-checkout list 2>/dev/null || echo '')"

  # Should start with /*
  if ! echo "$sparse_config" | grep -q '^/\*$'; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected sparse checkout to start with '/*'"
  fi

  # Should exclude windows files
  if ! echo "$sparse_config" | grep -q '!/symlinks/windows\.txt'; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected sparse checkout to exclude windows.txt"
  fi

  # Should exclude desktop files
  if ! echo "$sparse_config" | grep -q '!/symlinks/desktop\.txt'; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected sparse checkout to exclude desktop.txt"
  fi

  # Should NOT exclude arch files (arch is included)
  if echo "$sparse_config" | grep -q '!/symlinks/arch\.txt'; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Did not expect sparse checkout to exclude arch.txt"
  fi

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Pattern generation passed"
)}

# test_sparse_checkout_unchanged_config_skipped
#
# Test that unchanged configuration is skipped (idempotency)
test_sparse_checkout_unchanged_config_skipped()
{(
  log_stage "Testing unchanged config is skipped"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR and OPT
  DIR="$test_repo"
  OPT=" --skip-os-detection -v "
  export EXCLUDED_CATEGORIES=""

  # Configure sparse checkout first time
  local output1
  output1="$(configure_sparse_checkout "arch" 2>&1)"

  # Configure sparse checkout second time (should skip)
  local output2
  output2="$(configure_sparse_checkout "arch" 2>&1)"

  # Second run should mention skipping
  if ! echo "$output2" | grep -q "Skipping sparse checkout: configuration unchanged"; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected second run to skip unchanged configuration"
  fi

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Unchanged config skip passed"
)}

# test_sparse_checkout_config_change_detected
#
# Test that configuration changes are detected correctly
test_sparse_checkout_config_change_detected()
{(
  log_stage "Testing config change detection"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR and OPT
  DIR="$test_repo"
  OPT=" --skip-os-detection -v "
  export EXCLUDED_CATEGORIES=""

  # Configure with arch profile
  configure_sparse_checkout "arch"

  # Get initial config
  local config1
  config1="$(git -C "$test_repo" sparse-checkout list 2>/dev/null)"

  # Configure with different profile (base)
  configure_sparse_checkout "base"

  # Get new config
  local config2
  config2="$(git -C "$test_repo" sparse-checkout list 2>/dev/null)"

  # Configs should be different
  if [ "$config1" = "$config2" ]; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected sparse checkout config to change between profiles"
  fi

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Config change detection passed"
)}

# test_sparse_checkout_manifest_interaction
#
# Test interaction with manifest.ini for file exclusion lists
test_sparse_checkout_manifest_interaction()
{(
  log_stage "Testing manifest.ini interaction"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR and OPT
  DIR="$test_repo"
  OPT=" --skip-os-detection "
  export EXCLUDED_CATEGORIES=""

  # Configure with arch profile (excludes windows,desktop)
  configure_sparse_checkout "arch"

  # Check sparse checkout list includes exclusions from manifest
  local sparse_config
  sparse_config="$(git -C "$test_repo" sparse-checkout list 2>/dev/null)"

  # Should exclude files from windows section in manifest
  if ! echo "$sparse_config" | grep -q '!/symlinks/windows\.txt'; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected manifest.ini windows files to be excluded"
  fi

  # Should exclude files from desktop section in manifest
  if ! echo "$sparse_config" | grep -q '!/symlinks/desktop\.txt'; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected manifest.ini desktop files to be excluded"
  fi

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Manifest interaction passed"
)}

# test_sparse_checkout_not_git_repository
#
# Test handling of non-git repository
test_sparse_checkout_not_git_repository()
{(
  log_stage "Testing non-git repository handling"

  # Create temporary non-git directory
  local test_dir
  test_dir="$(mktemp -d)"
  trap 'rm -rf "$test_dir"' EXIT

  # Save original DIR
  local original_dir="$DIR"

  # Create conf directory but no .git
  mkdir -p "$test_dir/conf"
  _create_test_profiles_ini "$test_dir"

  # Set DIR to non-git directory
  DIR="$test_dir"
  export EXCLUDED_CATEGORIES=""

  # Should succeed but skip sparse checkout
  configure_sparse_checkout "base"

  # EXCLUDED_CATEGORIES should be empty
  if [ -n "$EXCLUDED_CATEGORIES" ]; then
    DIR="$original_dir"
    log_error "Expected empty EXCLUDED_CATEGORIES for non-git repo, got: $EXCLUDED_CATEGORIES"
  fi

  # Restore DIR
  DIR="$original_dir"

  log_verbose "Non-git repository handling passed"
)}

# test_sparse_checkout_uncommitted_changes_docker
#
# Test handling of uncommitted changes (Docker scenario)
test_sparse_checkout_uncommitted_changes_docker()
{(
  log_stage "Testing uncommitted changes handling"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Add uncommitted changes
  echo "uncommitted" > "$test_repo/symlinks/new-file.txt"

  # Set DIR and OPT
  DIR="$test_repo"
  OPT=" --skip-os-detection "
  export EXCLUDED_CATEGORIES=""

  # Configure sparse checkout (should auto-commit changes)
  configure_sparse_checkout "base"

  # Check that changes were committed (no uncommitted or staged changes remain)
  if ! git -C "$test_repo" diff --quiet; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected uncommitted changes to be auto-committed (working tree not clean)"
  fi

  if ! git -C "$test_repo" diff --cached --quiet; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected staged changes to be auto-committed (index not clean)"
  fi

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Uncommitted changes handling passed"
)}

# test_sparse_checkout_git_reset_sequence
#
# Test git reset --hard sequence
test_sparse_checkout_git_reset_sequence()
{(
  log_stage "Testing git reset sequence"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR and OPT
  DIR="$test_repo"
  OPT=" --skip-os-detection "
  export EXCLUDED_CATEGORIES=""

  # The arch.txt file exists from initial commit and should be removed by sparse checkout with base profile
  if [ ! -f "$test_repo/symlinks/arch.txt" ]; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected arch.txt to exist before sparse checkout"
  fi

  # Configure sparse checkout with base profile (excludes arch)
  configure_sparse_checkout "base"

  # File should be removed from working directory
  if [ -f "$test_repo/symlinks/arch.txt" ]; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected arch.txt to be removed by sparse checkout"
  fi

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Git reset sequence passed"
)}

# test_sparse_checkout_dry_run_no_modifications
#
# Test that dry-run mode doesn't modify git state
test_sparse_checkout_dry_run_no_modifications()
{(
  log_stage "Testing dry-run mode doesn't modify state"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Create a file that would be removed by sparse checkout
  echo "test" > "$test_repo/symlinks/arch.txt"
  git -C "$test_repo" add symlinks/arch.txt
  git -C "$test_repo" commit -q -m "Add test file"

  # Set DIR and OPT with dry-run
  DIR="$test_repo"
  OPT=" --skip-os-detection --dry-run "
  export EXCLUDED_CATEGORIES=""

  # Configure sparse checkout in dry-run mode
  local output
  output="$(configure_sparse_checkout "base" 2>&1)"

  # Should log dry-run message
  if ! echo "$output" | grep -q "DRY-RUN: Would apply sparse checkout"; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected dry-run message in output"
  fi

  # The symlinks directory should not have been deleted by rm -rf
  # (lines 540-543 are skipped in dry-run mode)
  if [ ! -d "$test_repo/symlinks" ]; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected symlinks directory to still exist (rm -rf skipped in dry-run)"
  fi

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Dry-run no modifications passed"
)}

# test_sparse_checkout_dry_run_validates_config
#
# Test that dry-run still validates configuration
test_sparse_checkout_dry_run_validates_config()
{(
  log_stage "Testing dry-run validates config"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"

  # Set DIR and OPT with dry-run
  DIR="$test_repo"
  OPT=" --skip-os-detection --dry-run "
  export EXCLUDED_CATEGORIES=""

  # Test with invalid profile (should still fail in dry-run)
  # Run in subshell to prevent exit from killing the test
  local exit_code=0
  (configure_sparse_checkout "invalid-profile") 2>/dev/null || exit_code=$?

  if [ "$exit_code" -eq 0 ]; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected error for invalid profile even in dry-run mode"
  fi

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Dry-run validates config passed"
)}

# test_sparse_checkout_idempotent
#
# Test that running configure_sparse_checkout multiple times is idempotent
test_sparse_checkout_idempotent()
{(
  log_stage "Testing idempotency"

  # Create temporary repo
  local test_repo
  test_repo="$(mktemp -d)"
  trap 'rm -rf "$test_repo"' EXIT

  # Save original variables
  local original_dir="$DIR"
  local original_opt="$OPT"

  # Setup test repo
  _create_test_repo "$test_repo"
  _create_test_profiles_ini "$test_repo"
  _create_test_manifest_ini "$test_repo"

  # Set DIR and OPT
  DIR="$test_repo"
  OPT=" --skip-os-detection "
  export EXCLUDED_CATEGORIES=""

  # Configure sparse checkout first time
  configure_sparse_checkout "arch"
  local config1
  config1="$(git -C "$test_repo" sparse-checkout list 2>/dev/null)"
  local excluded1="$EXCLUDED_CATEGORIES"

  # Configure sparse checkout second time
  configure_sparse_checkout "arch"
  local config2
  config2="$(git -C "$test_repo" sparse-checkout list 2>/dev/null)"
  local excluded2="$EXCLUDED_CATEGORIES"

  # Configure sparse checkout third time
  configure_sparse_checkout "arch"
  local config3
  config3="$(git -C "$test_repo" sparse-checkout list 2>/dev/null)"
  local excluded3="$EXCLUDED_CATEGORIES"

  # All configs should be identical
  if [ "$config1" != "$config2" ] || [ "$config2" != "$config3" ]; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected sparse checkout config to be idempotent across multiple runs"
  fi

  # All EXCLUDED_CATEGORIES should be identical
  if [ "$excluded1" != "$excluded2" ] || [ "$excluded2" != "$excluded3" ]; then
    DIR="$original_dir"; OPT="$original_opt"
    log_error "Expected EXCLUDED_CATEGORIES to be idempotent across multiple runs"
  fi

  # Restore variables
  DIR="$original_dir"
  OPT="$original_opt"

  log_verbose "Idempotency passed"
)}
