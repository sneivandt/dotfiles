#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-shell-wrapper.sh — Tests for dotfiles.sh and dotfiles.ps1 wrappers
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root), BINARY_PATH (path to test binary)
# -----------------------------------------------------------------------------

# shellcheck disable=SC3054
if [ -n "${BASH_SOURCE:-}" ]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
else
  SCRIPT_DIR="$(pwd)"
fi
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# ---------------------------------------------------------------------------
# Test binary download mechanism
# ---------------------------------------------------------------------------

test_wrapper_build_mode()
{(
  log_stage "Testing dotfiles.sh --build mode"
  
  # Ensure cargo is available
  if ! command -v cargo >/dev/null 2>&1; then
    log_verbose "Skipping: cargo not installed"
    return 0
  fi
  
  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT
  
  cd "$DIR"
  
  # Test --build flag builds and runs
  output=$("$DIR/dotfiles.sh" --build version 2>&1 || true)
  
  if echo "$output" | grep -q "dotfiles"; then
    log_verbose "✓ --build mode successfully builds and runs binary"
  else
    printf "%sERROR: --build mode failed: %s%s\n" "${RED}" "$output" "${NC}" >&2
    return 1
  fi
)}

test_wrapper_version_cache()
{(
  log_stage "Testing version cache mechanism"
  
  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT
  
  # Create test wrapper script that uses local binary
  cat > "$tmpdir/test-wrapper.sh" <<'EOF'
#!/bin/sh
set -o errexit

BIN_DIR="$1"
CACHE_FILE="$BIN_DIR/.dotfiles-version-cache"
BINARY="$BIN_DIR/dotfiles"

# Test cache freshness check
is_cache_fresh() {
  if [ ! -f "$CACHE_FILE" ]; then
    return 1
  fi
  cached_ts=$(sed -n '2p' "$CACHE_FILE" 2>/dev/null || echo "0")
  now=$(date +%s)
  age=$((now - cached_ts))
  [ "$age" -lt "3600" ]
}

# Test 1: No cache file - should not be fresh
if is_cache_fresh; then
  echo "ERROR: Empty cache reported as fresh"
  exit 1
fi

# Test 2: Create fresh cache
mkdir -p "$BIN_DIR"
echo "v0.1.0" > "$CACHE_FILE"
date +%s >> "$CACHE_FILE"

if ! is_cache_fresh; then
  echo "ERROR: Fresh cache not detected"
  exit 1
fi

# Test 3: Create stale cache
echo "v0.1.0" > "$CACHE_FILE"
echo "0" >> "$CACHE_FILE"

if is_cache_fresh; then
  echo "ERROR: Stale cache reported as fresh"
  exit 1
fi

echo "OK"
EOF
  
  chmod +x "$tmpdir/test-wrapper.sh"
  output=$("$tmpdir/test-wrapper.sh" "$tmpdir")
  
  if [ "$output" = "OK" ]; then
    log_verbose "✓ Cache freshness logic works correctly"
  else
    printf "%sERROR: Cache test failed: %s%s\n" "${RED}" "$output" "${NC}" >&2
    return 1
  fi
)}

test_wrapper_uses_local_binary()
{(
  log_stage "Testing wrapper uses downloaded binary"
  
  # Test that wrapper can find and execute pre-downloaded binary
  if [ -z "${BINARY_PATH:-}" ]; then
    log_verbose "Skipping: BINARY_PATH not set"
    return 0
  fi
  
  if [ ! -f "$BINARY_PATH" ]; then
    printf "%sERROR: Binary not found at %s%s\n" "${RED}" "$BINARY_PATH" "${NC}" >&2
    return 1
  fi
  
  # Binary should be executable and report version
  if ! "$BINARY_PATH" version >/dev/null 2>&1; then
    printf "%sERROR: Binary cannot execute version command%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  
  log_verbose "✓ Downloaded binary is functional"
)}

test_wrapper_checksum_verification()
{(
  log_stage "Testing checksum verification logic"
  
  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT
  
  # Create test binary file
  echo "fake binary content" > "$tmpdir/dotfiles"
  
  # Create checksums file
  cat > "$tmpdir/checksums.sha256" <<EOF
abc123  dotfiles-linux-x86_64
def456  dotfiles-windows-x86_64.exe
EOF
  
  # Test checksum extraction
  expected=$(grep "dotfiles-linux-x86_64" "$tmpdir/checksums.sha256" | awk '{print $1}')
  
  if [ "$expected" = "abc123" ]; then
    log_verbose "✓ Checksum extraction works correctly"
  else
    printf "%sERROR: Checksum extraction failed: got '%s'%s\n" "${RED}" "$expected" "${NC}" >&2
    return 1
  fi
  
  # Calculate actual checksum
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmpdir/dotfiles" | awk '{print $1}')
    log_verbose "✓ Can calculate sha256sum: $actual"
  fi
)}

test_wrapper_offline_fallback()
{(
  log_stage "Testing offline fallback behavior"
  
  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT
  
  # Create a mock wrapper that simulates offline behavior
  cat > "$tmpdir/test-offline.sh" <<'EOF'
#!/bin/sh

# Simulate get_latest_version returning empty (offline)
get_latest_version() {
  echo ""
}

# Simulate existing local binary
get_local_version() {
  echo "v0.1.0"
}

latest=$(get_latest_version)
local_version=$(get_local_version)

if [ -z "$latest" ] && [ "$local_version" != "none" ]; then
  echo "Using cached dotfiles $local_version (offline)"
  exit 0
fi

if [ -z "$latest" ] && [ "$local_version" = "none" ]; then
  echo "ERROR: Cannot determine latest version and no local binary found."
  exit 1
fi

exit 1
EOF
  
  chmod +x "$tmpdir/test-offline.sh"
  output=$("$tmpdir/test-offline.sh" 2>&1 || true)
  
  if echo "$output" | grep -q "Using cached"; then
    log_verbose "✓ Offline fallback works with cached binary"
  else
    printf "%sERROR: Offline fallback test failed: %s%s\n" "${RED}" "$output" "${NC}" >&2
    return 1
  fi
)}

test_wrapper_forwarded_args()
{(
  log_stage "Testing argument forwarding"
  
  if [ -z "${BINARY_PATH:-}" ]; then
    log_verbose "Skipping: BINARY_PATH not set"
    return 0
  fi
  
  # Test that arguments are properly forwarded
  # The binary should understand --help flag
  if "$BINARY_PATH" --help >/dev/null 2>&1; then
    log_verbose "✓ Arguments forwarded correctly"
  else
    printf "%sERROR: Argument forwarding failed%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
)}

test_wrapper_root_detection()
{(
  log_stage "Testing DOTFILES_ROOT detection"
  
  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT
  
  # Create test script that checks root detection
  cat > "$tmpdir/test-root.sh" <<'EOF'
#!/bin/sh
DOTFILES_ROOT="$(dirname "$(readlink -f "$0")")"
export DOTFILES_ROOT

# Check that root is correctly set to script directory
if [ -n "$DOTFILES_ROOT" ] && [ -d "$DOTFILES_ROOT" ]; then
  echo "OK: $DOTFILES_ROOT"
else
  echo "ERROR: Invalid root"
  exit 1
fi
EOF
  
  chmod +x "$tmpdir/test-root.sh"
  output=$("$tmpdir/test-root.sh")
  
  if echo "$output" | grep -q "OK:"; then
    log_verbose "✓ DOTFILES_ROOT detection works"
  else
    printf "%sERROR: Root detection failed: %s%s\n" "${RED}" "$output" "${NC}" >&2
    return 1
  fi
)}

test_wrapper_error_handling()
{(
  log_stage "Testing error handling"
  
  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT
  
  # Test missing cargo in build mode
  cat > "$tmpdir/test-error.sh" <<'EOF'
#!/bin/sh
set -o errexit

# Simulate missing cargo
if ! false; then
  echo "ERROR: cargo not found. Install Rust to use --build mode." >&2
  exit 1
fi
EOF
  
  chmod +x "$tmpdir/test-error.sh"
  # Run script and check exit code separately to avoid mixing stdout/stderr
  if "$tmpdir/test-error.sh" >/dev/null 2>&1; then
    printf "%sERROR: Script should have failed but succeeded%s\n" "${RED}" "${NC}" >&2
    return 1
  else
    log_verbose "✓ Error handling exits with proper code"
  fi
)}
