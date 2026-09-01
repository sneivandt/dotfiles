#!/bin/sh
set -o errexit
set -o nounset

# -----------------------------------------------------------------------------
# test-shell-wrapper.sh — Tests for dotfiles.sh and dotfiles.ps1 wrappers
# Dependencies: test-helpers.sh
# Expected:     DIR (repository root), BINARY_PATH (path to test binary)
# -----------------------------------------------------------------------------

SCRIPT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
# shellcheck source=lib/test-helpers.sh
. "$SCRIPT_DIR"/lib/test-helpers.sh

# ---------------------------------------------------------------------------
# Test binary download mechanism
# ---------------------------------------------------------------------------

test_wrapper_build_mode()
{(
  log_stage "Testing dotfiles.sh --build mode"

  # Skip if pre-built binary is available (build tested separately in CI)
  if [ -n "$BINARY_PATH" ] && [ -x "$BINARY_PATH" ]; then
    log_verbose "Skipping: pre-built binary available, build tested separately"
    return 0
  fi

  # Ensure cargo is available
  if ! command -v cargo >/dev/null 2>&1; then
    log_verbose "Skipping: cargo not installed"
    return 0
  fi

  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT

  cd "$DIR"

  # Test --build flag builds and runs
  output=$("$DIR/dotfiles.sh" --build --version 2>&1 || true)

  if echo "$output" | grep -q "dotfiles"; then
    log_verbose "✓ --build mode successfully builds and runs binary"
  else
    printf "%sERROR: --build mode failed: %s%s\n" "${RED}" "$output" "${NC}" >&2
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
  if ! "$BINARY_PATH" --version >/dev/null 2>&1; then
    printf "%sERROR: Binary cannot execute version command%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  log_verbose "✓ Downloaded binary is functional"
)}

test_wrapper_forwarded_args()
{(
  log_stage "Testing argument forwarding"

  if [ -z "${BINARY_PATH:-}" ]; then
    log_verbose "Skipping: BINARY_PATH not set"
    return 0
  fi

  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT

  cp "$DIR/dotfiles.sh" "$tmpdir/dotfiles.sh"
  mkdir -p "$tmpdir/bin"
  cp "$BINARY_PATH" "$tmpdir/bin/dotfiles"
  chmod +x "$tmpdir/bin/dotfiles"

  # Validate wrapper argument forwarding by executing through dotfiles.sh.
  if "$tmpdir/dotfiles.sh" --version >/dev/null 2>&1; then
    log_verbose "✓ Arguments forwarded correctly"
  else
    printf "%sERROR: Argument forwarding failed%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
)}

test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args()
{(
  log_stage "Testing real wrapper bootstrap download, checksum, and forwarding"

  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT

  cp "$DIR/dotfiles.sh" "$tmpdir/dotfiles.sh"
  mkdir -p "$tmpdir/fake-bin"

  cat > "$tmpdir/fake-bin/curl" <<'EOF'
#!/bin/sh
set -o errexit
set -o nounset

out=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      shift
      out="$1"
      ;;
    http*)
      url="$1"
      ;;
  esac
  shift
done

case "$url" in
  */releases/latest)
    printf '{"tag_name":"v9.9.9"}\n'
    ;;
  */releases/download/v9.9.9/checksums.sha256)
    sum=$(sha256sum "$DOTFILES_ROOT/bin/dotfiles" | awk '{print $1}')
    printf '%s  dotfiles-linux-x86_64\n' "$sum" > "$out"
    ;;
  */releases/download/v9.9.9/dotfiles-linux-x86_64)
    mkdir -p "$(dirname "$out")"
    cat > "$out" <<'BIN'
#!/bin/sh
printf '%s\n' "$DOTFILES_ROOT" > "$DOTFILES_ROOT/root.txt"
printf '%s\n' "$DOTFILES_WRAPPER" > "$DOTFILES_ROOT/wrapper.txt"
printf '%s\n' "$@" > "$DOTFILES_ROOT/args.txt"
BIN
    ;;
  *)
    echo "unexpected URL: $url" >&2
    exit 1
    ;;
esac
EOF
  chmod +x "$tmpdir/fake-bin/curl"

  PATH="$tmpdir/fake-bin:$PATH" DOTFILES_SKIP_ATTESTATION=1 \
    "$tmpdir/dotfiles.sh" install -p desktop -n

  expected=$(cat <<'EOF'
install
-p
desktop
-n
EOF
)
  actual=$(cat "$tmpdir/args.txt")
  root=$(cat "$tmpdir/root.txt")
  wrapper=$(cat "$tmpdir/wrapper.txt")

  if [ "$actual" != "$expected" ]; then
    printf "%sERROR: Bootstrap wrapper forwarded args mismatch.%s\nExpected:\n%s\nActual:\n%s\n" "${RED}" "${NC}" "$expected" "$actual" >&2
    return 1
  fi
  if [ "$root" != "$tmpdir" ]; then
    printf "%sERROR: DOTFILES_ROOT mismatch: expected '%s', got '%s'%s\n" "${RED}" "$tmpdir" "$root" "${NC}" >&2
    return 1
  fi
  if [ "$wrapper" != "sh" ]; then
    printf "%sERROR: DOTFILES_WRAPPER mismatch: expected 'sh', got '%s'%s\n" "${RED}" "$wrapper" "${NC}" >&2
    return 1
  fi
  if [ ! -x "$tmpdir/bin/dotfiles" ]; then
    printf "%sERROR: downloaded binary was not made executable%s\n" "${RED}" "${NC}" >&2
    return 1
  fi

  log_verbose "✓ Real wrapper bootstrap downloads, verifies, chmods, and forwards arguments"
)}

test_wrapper_build_mode_consumes_build_flag_and_forwards_cli_args()
{(
  log_stage "Testing build-mode consumes --build and forwards CLI arguments"

  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT

  cp "$DIR/dotfiles.sh" "$tmpdir/dotfiles.sh"
  mkdir -p "$tmpdir/cli/target/dev-opt" "$tmpdir/fake-bin"

  cat > "$tmpdir/fake-bin/cargo" <<'EOF'
#!/bin/sh
exit 0
EOF
  chmod +x "$tmpdir/fake-bin/cargo"

  cat > "$tmpdir/cli/target/dev-opt/dotfiles" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" > "$DOTFILES_ROOT/forwarded-args.txt"
EOF
  chmod +x "$tmpdir/cli/target/dev-opt/dotfiles"

  PATH="$tmpdir/fake-bin:$PATH" "$tmpdir/dotfiles.sh" --build install -p desktop -n -v

  expected=$(cat <<EOF
install
-p
desktop
-n
-v
EOF
)
  actual=$(cat "$tmpdir/forwarded-args.txt")

  if [ "$actual" = "$expected" ]; then
    log_verbose "✓ Build mode consumes --build and forwards CLI arguments"
  else
    printf "%sERROR: Forwarded args mismatch.%s\nExpected:\n%s\nActual:\n%s\n" "${RED}" "${NC}" "$expected" "$actual" >&2
    return 1
  fi
)}

test_wrapper_forwards_advanced_flags()
{(
  log_stage "Testing wrapper forwards advanced flags"

  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT

  cp "$DIR/dotfiles.sh" "$tmpdir/dotfiles.sh"
  mkdir -p "$tmpdir/cli/target/dev-opt" "$tmpdir/fake-bin"

  cat > "$tmpdir/fake-bin/cargo" <<'EOF'
#!/bin/sh
exit 0
EOF
  chmod +x "$tmpdir/fake-bin/cargo"

  cat > "$tmpdir/cli/target/dev-opt/dotfiles" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" > "$DOTFILES_ROOT/forwarded-advanced-args.txt"
EOF
  chmod +x "$tmpdir/cli/target/dev-opt/dotfiles"

  PATH="$tmpdir/fake-bin:$PATH" \
    "$tmpdir/dotfiles.sh" --build install --skip symlinks --only packages --no-parallel

  expected=$(cat <<'EOF'
install
--skip
symlinks
--only
packages
--no-parallel
EOF
)
  actual=$(cat "$tmpdir/forwarded-advanced-args.txt")

  if [ "$actual" = "$expected" ]; then
    log_verbose "✓ Wrapper forwards advanced flags to the Rust CLI"
  else
    printf "%sERROR: Advanced flag forwarding mismatch.%s\nExpected:\n%s\nActual:\n%s\n" "${RED}" "${NC}" "$expected" "$actual" >&2
    return 1
  fi
)}

test_wrapper_chmod_after_checksum()
{(
  log_stage "Testing chmod +x occurs after checksum verification"

  wrapper="$DIR/dotfiles.sh"

  # Extract the line numbers of the two operations inside download_binary so
  # we can assert that checksum verification precedes chmod +x.  This is a
  # source-level guard that prevents a TOCTOU regression where a downloaded
  # binary becomes executable before its integrity has been confirmed.
  # Match the invocation line (contains a quoted argument) to skip the
  # function-definition line.
  verify_line=$(grep -n '_verify_checksum "' "$wrapper" | head -1 | cut -d: -f1)
  chmod_line=$(grep -n "chmod +x" "$wrapper" | head -1 | cut -d: -f1)

  if [ -z "$verify_line" ] || [ -z "$chmod_line" ]; then
    printf "%sERROR: Could not locate _verify_checksum or chmod +x in %s%s\n" \
      "${RED}" "$wrapper" "${NC}" >&2
    return 1
  fi

  if [ "$chmod_line" -gt "$verify_line" ]; then
    log_verbose "✓ chmod +x (line $chmod_line) appears after _verify_checksum (line $verify_line)"
  else
    printf "%sERROR: chmod +x (line %s) must come after _verify_checksum (line %s)%s\n" \
      "${RED}" "$chmod_line" "$verify_line" "${NC}" >&2
    return 1
  fi
)}

test_wrapper_attestation_verification()
{(
  log_stage "Testing build provenance verification during bootstrap download"

  tmpdir=$(mktemp -d)
  trap 'rm -rf "$tmpdir"' EXIT

  cp "$DIR/dotfiles.sh" "$tmpdir/dotfiles.sh"
  mkdir -p "$tmpdir/fake-bin"

  cat > "$tmpdir/fake-bin/curl" <<'EOF'
#!/bin/sh
set -o errexit
set -o nounset

out=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o)
      shift
      out="$1"
      ;;
    http*)
      url="$1"
      ;;
  esac
  shift
done

case "$url" in
  */releases/latest)
    printf '{"tag_name":"v9.9.9"}\n'
    ;;
  */releases/download/v9.9.9/checksums.sha256)
    sum=$(sha256sum "$DOTFILES_ROOT/bin/dotfiles" | awk '{print $1}')
    printf '%s  dotfiles-linux-x86_64\n' "$sum" > "$out"
    ;;
  */releases/download/v9.9.9/dotfiles-linux-x86_64)
    mkdir -p "$(dirname "$out")"
    printf '#!/bin/sh\nexit 0\n' > "$out"
    ;;
  *)
    echo "unexpected URL: $url" >&2
    exit 1
    ;;
esac
EOF
  chmod +x "$tmpdir/fake-bin/curl"

  # A gh stub that never reports a verified attestation.
  cat > "$tmpdir/fake-bin/gh" <<'EOF'
#!/bin/sh
exit 1
EOF
  chmod +x "$tmpdir/fake-bin/gh"

  # Required (default): unverified provenance is fatal and removes the download.
  rm -rf "${tmpdir:?}/bin"
  if PATH="$tmpdir/fake-bin:$PATH" "$tmpdir/dotfiles.sh" --version >/dev/null 2>&1; then
    printf "%sERROR: Default policy accepted an unverified binary%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  if [ -e "$tmpdir/bin/dotfiles" ]; then
    printf "%sERROR: Unverified binary was not removed by default%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  log_verbose "✓ Default policy rejects unverified downloads"

  # Skip: verification is bypassed entirely.
  rm -rf "${tmpdir:?}/bin"
  PATH="$tmpdir/fake-bin:$PATH" DOTFILES_SKIP_ATTESTATION=1 \
    "$tmpdir/dotfiles.sh" --version >/dev/null 2>&1
  if [ ! -x "$tmpdir/bin/dotfiles" ]; then
    printf "%sERROR: DOTFILES_SKIP_ATTESTATION=1 did not bypass verification%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  log_verbose "✓ DOTFILES_SKIP_ATTESTATION=1 bypasses provenance verification"

  # Missing gh: warn and continue so a fresh bootstrap can install it.
  rm -rf "${tmpdir:?}/bin"
  rm -f "$tmpdir/fake-bin/gh"
  for command_name in awk chmod dirname mkdir mktemp readlink rm sha256sum uname; do
    ln -s "$(command -v "$command_name")" "$tmpdir/fake-bin/$command_name"
  done
  output=$(PATH="$tmpdir/fake-bin" "$tmpdir/dotfiles.sh" --version 2>&1)
  if [ ! -x "$tmpdir/bin/dotfiles" ]; then
    printf "%sERROR: Missing gh prevented bootstrap%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  if ! printf '%s\n' "$output" | grep -q \
    "WARNING: gh not found. Skipping build provenance verification."; then
    printf "%sERROR: Missing gh did not produce the expected warning%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  log_verbose "✓ Missing gh warns without blocking bootstrap"
)}

test_wrapper_release_pinned_urls()
{(
  log_stage "Testing wrapper resolves release tag and uses pinned URLs for binary and checksum"

  wrapper="$DIR/dotfiles.sh"
  content=$(cat "$wrapper")

  # shellcheck disable=SC2016
  if echo "$content" | grep -q "resolve_release_tag" && \
     echo "$content" | grep -q 'releases/download/\$tag' && \
     ! echo "$content" | grep -q 'releases/latest/download'; then
    log_verbose "✓ Wrapper resolves release tag and uses pinned URLs for binary and checksum"
  else
    printf "%sERROR: Wrapper does not use version-pinned URLs for bootstrap downloads%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
)}

# Run all tests when executed directly
case "$0" in
  *test-shell-wrapper.sh)
    test_wrapper_build_mode
    test_wrapper_uses_local_binary
    test_wrapper_forwarded_args
    test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args
    test_wrapper_build_mode_consumes_build_flag_and_forwards_cli_args
    test_wrapper_forwards_advanced_flags
    test_wrapper_chmod_after_checksum
    test_wrapper_attestation_verification
    test_wrapper_release_pinned_urls
    echo "All shell wrapper tests passed"
    ;;
esac
