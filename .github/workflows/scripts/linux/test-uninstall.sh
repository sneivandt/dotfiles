#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-uninstall.sh — Tests for the install/uninstall command round-trip.
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root), BINARY_PATH (path to pre-built binary)
# -----------------------------------------------------------------------------

# shellcheck disable=SC3054
if [ -n "${BASH_SOURCE:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
else
  SCRIPT_DIR="$(pwd)"
fi
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# Verify that a path is a symlink to the expected repository source.
assert_symlink() {
  path="$1"
  expected="$2"
  if [ ! -L "$path" ]; then
    printf "%sERROR: expected symlink: %s%s\n" "${RED}" "$path" "${NC}" >&2
    return 1
  fi
  actual_target="$(readlink -f "$path" 2>/dev/null || true)"
  expected_target="$(readlink -f "$expected" 2>/dev/null || true)"
  if [ -z "$actual_target" ] || [ "$actual_target" != "$expected_target" ]; then
    printf "%sERROR: symlink %s points to '%s', expected '%s'%s\n" "${RED}" "$path" "$actual_target" "$expected_target" "${NC}" >&2
    return 1
  fi
  log_verbose "✓ symlink target: $path → $expected_target"
}

# Verify that a path is materialized with the expected content.
assert_materialized() {
  path="$1"
  expected="$2"
  snapshot="$3"
  if [ ! -f "$path" ]; then
    printf "%sERROR: expected file after uninstall: %s%s\n" "${RED}" "$path" "${NC}" >&2
    return 1
  fi
  if [ -L "$path" ]; then
    printf "%sERROR: expected materialized file, still a symlink: %s%s\n" "${RED}" "$path" "${NC}" >&2
    return 1
  fi
  if ! cmp -s "$path" "$expected" || ! cmp -s "$path" "$snapshot"; then
    printf "%sERROR: materialized content does not match the installed source: %s%s\n" "${RED}" "$path" "${NC}" >&2
    return 1
  fi
  log_verbose "✓ materialized content: $path"
}

test_assertion_helpers() {
  fixture="$(mktemp -d)"
  trap 'rm -rf "$fixture"' EXIT
  printf 'expected\n' > "$fixture/source"
  printf 'wrong\n' > "$fixture/other"
  ln -s "$fixture/source" "$fixture/link"

  assert_symlink "$fixture/link" "$fixture/source"
  rm "$fixture/link"
  ln -s "$fixture/other" "$fixture/link"
  if assert_symlink "$fixture/link" "$fixture/source" 2>/dev/null; then
    log_error "assert_symlink accepted the wrong target"
  fi
  rm "$fixture/link"
  ln -s "$fixture/missing" "$fixture/link"
  if assert_symlink "$fixture/link" "$fixture/source" 2>/dev/null; then
    log_error "assert_symlink accepted a dangling target"
  fi

  cp "$fixture/source" "$fixture/materialized"
  cp "$fixture/source" "$fixture/snapshot"
  assert_materialized "$fixture/materialized" "$fixture/source" "$fixture/snapshot"
  printf 'changed\n' > "$fixture/materialized"
  if assert_materialized "$fixture/materialized" "$fixture/source" "$fixture/snapshot" 2>/dev/null; then
    log_error "assert_materialized accepted changed content"
  fi

  rm -rf "$fixture"
  trap - EXIT
}

# Test the full install → uninstall round-trip for the base profile.
test_install_uninstall_base_profile()
{(
  log_stage "Testing install/uninstall round-trip (base profile)"

  [ -n "${BINARY_PATH:-}" ] || log_error "BINARY_PATH is not set"
  [ -f "$BINARY_PATH" ] || log_error "Binary not found: $BINARY_PATH"

  # Run install
  log_verbose "Running install..."
  "$BINARY_PATH" --root "$DIR" -p base install --skip apm
  log_verbose "Install complete"

  # Verify representative symlinks were created
  assert_symlink "$HOME/.bashrc" "$DIR/symlinks/bashrc"
  assert_symlink "$HOME/.zshrc" "$DIR/symlinks/zshrc"
  assert_symlink "$HOME/.config/git/config" "$DIR/symlinks/config/git/config"

  snapshot="$(mktemp -d)"
  trap 'rm -rf "$snapshot"' EXIT
  cp -L "$HOME/.bashrc" "$snapshot/bashrc"
  cp -L "$HOME/.zshrc" "$snapshot/zshrc"
  cp -L "$HOME/.config/git/config" "$snapshot/git-config"

  # Run uninstall
  log_verbose "Running uninstall..."
  "$BINARY_PATH" --root "$DIR" -p base uninstall
  log_verbose "Uninstall complete"

  # After uninstall symlinks should be materialized as real files
  assert_materialized "$HOME/.bashrc" "$DIR/symlinks/bashrc" "$snapshot/bashrc"
  assert_materialized "$HOME/.zshrc" "$DIR/symlinks/zshrc" "$snapshot/zshrc"
  assert_materialized "$HOME/.config/git/config" "$DIR/symlinks/config/git/config" "$snapshot/git-config"
)}

# Run all tests when executed directly
case "$0" in
  *test-uninstall.sh)
    test_assertion_helpers
    test_install_uninstall_base_profile
    echo "All uninstall tests passed"
    ;;
esac
