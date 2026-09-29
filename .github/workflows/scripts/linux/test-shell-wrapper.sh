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

  tmpdir="$DIR/.wrapper-bootstrap-$$"
  mkdir "$tmpdir"
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
    -o|-qO)
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
    if [ "${WRAPPER_RELEASE_JSON+x}" = x ]; then
      printf '%s\n' "$WRAPPER_RELEASE_JSON"
    else
      printf '{"tag_name":"v9.9.9"}\n'
    fi
    exit "${WRAPPER_RELEASE_STATUS:-0}"
    ;;
  */releases/download/v9.9.9/checksums.sha256)
    [ ! -e "$DOTFILES_ROOT/bin/dotfiles" ] || {
      echo "bootstrap published its binary before verification" >&2
      exit 1
    }
    sum=$(sha256sum "$(cat "$DOTFILES_ROOT/download-path")" | awk '{print $1}')
    printf '%s  dotfiles-linux-x86_64\n' "$sum" > "$out"
    ;;
  */releases/download/v9.9.9/dotfiles-linux-x86_64)
    mkdir -p "$(dirname "$out")"
    printf '%s\n' "$out" > "$DOTFILES_ROOT/download-path"
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
  wrapper_path="$tmpdir/fake-bin:$PATH"
  if [ "${WRAPPER_DOWNLOADER:-curl}" = wget ]; then
    mv "$tmpdir/fake-bin/curl" "$tmpdir/fake-bin/wget"
    for command_name in awk cat chmod dirname mkdir mv readlink rm rmdir sha256sum uname; do
      ln -s "$(command -v "$command_name")" "$tmpdir/fake-bin/$command_name"
    done
    wrapper_path="$tmpdir/fake-bin"
  fi

  status=0
  PATH="$wrapper_path" DOTFILES_SKIP_ATTESTATION=1 \
    "$tmpdir/dotfiles.sh" install -p desktop -n > "$tmpdir/stdout" 2> "$tmpdir/stderr" || status=$?
  if [ "${WRAPPER_EXPECT_RELEASE_ERROR:-0}" = 1 ]; then
    [ "$status" -ne 0 ] || log_error "Wrapper accepted an unsuccessful release lookup"
    [ ! -e "$tmpdir/download-path" ] || log_error "Failed release lookup downloaded an asset"
    [ ! -e "$tmpdir/args.txt" ] || log_error "Failed release lookup executed a child"
    grep -q 'Failed to resolve latest release tag' "$tmpdir/stderr" ||
      log_error "Failed release lookup was not explained"
    return 0
  fi
  [ "$status" -eq 0 ] || log_error "Bootstrap failed: $(cat "$tmpdir/stderr")"

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

test_wrapper_release_metadata_formats()
{(
  log_stage "Testing release JSON property order and failed metadata transfers"
  for downloader in curl wget; do
    WRAPPER_DOWNLOADER="$downloader"
    export WRAPPER_DOWNLOADER
    WRAPPER_RELEASE_JSON='{"url":"https://api.github.com/example","tag_name":"v9.9.9","name":"release"}' \
      test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args
    WRAPPER_RELEASE_JSON='{
    "url": "https://api.github.com/example",
    "name": "release", "tag_name" : "v9.9.9"
  }' test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args
    WRAPPER_RELEASE_STATUS=23 WRAPPER_EXPECT_RELEASE_ERROR=1 \
      test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args
    WRAPPER_RELEASE_JSON='{"name":"release without a tag"}' WRAPPER_EXPECT_RELEASE_ERROR=1 \
      test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args
  done
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
printf '{"reason":"compiler-artifact","target":{"name":"dotfiles"},"executable":"%s/cli/target/dev-opt/dotfiles"}\n' "$DOTFILES_ROOT"
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
printf '{"reason":"compiler-artifact","target":{"name":"dotfiles"},"executable":"%s/cli/target/dev-opt/dotfiles"}\n' "$DOTFILES_ROOT"
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

test_wrapper_preserves_runtime_context()
{(
  log_stage "Testing cached and build-mode cwd, exact arguments, and failures"
  tmpdir="$DIR/.wrapper-context-$$"
  mkdir "$tmpdir"
  trap 'rm -rf "$tmpdir"' EXIT
  cp "$DIR/dotfiles.sh" "$tmpdir/dotfiles.sh"
  mkdir -p "$tmpdir/cli/target/dev-opt" "$tmpdir/bin" "$tmpdir/fake-bin" "$tmpdir/caller dir"
  cat > "$tmpdir/fake-bin/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "$PWD" > "$DOTFILES_ROOT/cargo-cwd"
printf '{"reason":"compiler-artifact","target":{"name":"dotfiles"},"executable":"%s/cli/target/dev-opt/dotfiles"}\n' "$DOTFILES_ROOT"
exit "${WRAPPER_CARGO_EXIT:-0}"
EOF
  cat > "$tmpdir/bin/dotfiles" <<'EOF'
#!/bin/sh
printf '%s\n' "$PWD" > "$DOTFILES_ROOT/child-cwd"
printf '%s\n' "$DOTFILES_ROOT" "$DOTFILES_WRAPPER" "$DOTFILES_REEXEC_GUARD" > "$DOTFILES_ROOT/child-context"
printf '%s\n' "$#" > "$DOTFILES_ROOT/child-argc"
printf '%s\n' "$@" > "$DOTFILES_ROOT/child-args"
printf 'child stdout\n'
printf 'child stderr\n' >&2
exit 7
EOF
  cp "$tmpdir/bin/dotfiles" "$tmpdir/cli/target/dev-opt/dotfiles"
  chmod +x "$tmpdir/fake-bin/cargo" "$tmpdir/bin/dotfiles" "$tmpdir/cli/target/dev-opt/dotfiles"
  PATH="$tmpdir/fake-bin:$PATH"
  DOTFILES_REEXEC_GUARD=1
  export PATH DOTFILES_REEXEC_GUARD
  cd "$tmpdir/caller dir"
  for mode in cached build; do
    set --
    [ "$mode" != build ] || set -- --build
    status=0
    "$tmpdir/dotfiles.sh" "$@" install --dry-run --root . --overlay "../overlay dir" "" --BUILD -- "literal value" --build "" \
      > "$tmpdir/stdout" 2> "$tmpdir/stderr" || status=$?
    [ "$status" -eq 7 ] || log_error "$mode wrapper lost the child exit code"
    [ "$(cat "$tmpdir/child-cwd")" = "$PWD" ] || log_error "$mode wrapper changed the child cwd"
    [ "$(cat "$tmpdir/child-context")" = "$(printf '%s\n' "$tmpdir" sh 1)" ] || log_error "$mode wrapper changed runtime context"
    expected=$(printf '%s\n' install --dry-run --root . --overlay "../overlay dir" "" --BUILD -- "literal value" --build "")
    [ "$(cat "$tmpdir/child-argc")" -eq 12 ] || log_error "$mode wrapper changed the argument count"
    [ "$(cat "$tmpdir/child-args")" = "$expected" ] || log_error "$mode wrapper changed arguments"
    [ "$(cat "$tmpdir/stdout")" = 'child stdout' ] || log_error "$mode wrapper changed stdout"
    [ "$(cat "$tmpdir/stderr")" = 'child stderr' ] || log_error "$mode wrapper changed stderr"
  done
  [ "$(cat "$tmpdir/cargo-cwd")" = "$tmpdir/cli" ] || log_error "Cargo did not run from cli/"
  rm "$tmpdir/child-cwd"
  status=0
  WRAPPER_CARGO_EXIT=23 "$tmpdir/dotfiles.sh" --build --version || status=$?
  [ "$status" -eq 23 ] || log_error "Wrapper lost the build failure exit code"
  [ ! -e "$tmpdir/child-cwd" ] || log_error "Wrapper ran the child after a failed build"
)}

test_wrapper_uses_cargo_artifact()
{(
  log_stage "Testing build-mode Cargo output directories and stale-artifact rejection"
  fixture="$DIR/.wrapper-artifact-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  cp "$DIR/dotfiles.sh" "$fixture/dotfiles.sh"
  mkdir -p "$fixture/cli/.cargo" "$fixture/cli/target/dev-opt" "$fixture/fake-bin" "$fixture/caller dir"
  cat > "$fixture/cli/target/dev-opt/dotfiles" <<'EOF'
#!/bin/sh
echo "ERROR: stale default-target binary executed" >&2
exit 91
EOF
  cat > "$fixture/fake-bin/cargo" <<'EOF'
#!/bin/sh
set -eu
[ "$*" = 'build --profile dev-opt --bin dotfiles --message-format=json-render-diagnostics' ]
target=${CARGO_TARGET_DIR:-$(sed -n 's/^target-dir = "\(.*\)"$/\1/p' .cargo/config.toml)}
case "$target" in /*) ;; *) target="$PWD/$target" ;; esac
executable="$target/custom-triple/dev-opt/dotfiles"
mkdir -p "$(dirname "$executable")"
cat > "$executable" <<'BIN'
#!/bin/sh
printf '%s\n' "$PWD" > "$DOTFILES_ROOT/child-cwd"
printf '%s\n' "$@" > "$DOTFILES_ROOT/child-args"
exit 7
BIN
chmod +x "$executable"
if [ "${WRAPPER_NO_ARTIFACT:-0}" != 1 ]; then
  escaped=$(printf '%s' "$executable" | sed 's/\\/\\\\/g; s/"/\\"/g; s,/,\\u002f,g; s/ /\\u0020/g')
  printf '{"reason":"compiler-artifact","target":{"name":"dotfiles"},"executable":"%s"}\n' "$escaped"
fi
printf '{"reason":"build-finished","success":true}\n'
EOF
  chmod +x "$fixture/fake-bin/cargo" "$fixture/cli/target/dev-opt/dotfiles"
  PATH="$fixture/fake-bin:$PATH"
  export PATH
  cd "$fixture/caller dir"
  for mode in environment config; do
    unset CARGO_TARGET_DIR
    if [ "$mode" = environment ]; then
      CARGO_TARGET_DIR="$fixture/environment output"
      export CARGO_TARGET_DIR
    else
      printf '[build]\ntarget-dir = "configured output"\n' > "$fixture/cli/.cargo/config.toml"
    fi
    status=0
    "$fixture/dotfiles.sh" --build --version 'space value' '' > "$fixture/stdout" 2> "$fixture/stderr" || status=$?
    [ "$status" -eq 7 ] || log_error "$mode did not execute Cargo artifact: $(cat "$fixture/stderr")"
    [ "$(cat "$fixture/child-cwd")" = "$PWD" ] || log_error "$mode changed the caller cwd"
    [ "$(cat "$fixture/child-args")" = "$(printf '%s\n' --version 'space value' '')" ] ||
      log_error "$mode changed child arguments"
    [ ! -s "$fixture/stdout" ] || log_error "$mode leaked Cargo JSON to stdout"
  done
  rm "$fixture/child-cwd"
  if WRAPPER_NO_ARTIFACT=1 "$fixture/dotfiles.sh" --build --version > "$fixture/stdout" 2> "$fixture/stderr"; then
    log_error "Missing Cargo artifact did not fail"
  fi
  [ ! -e "$fixture/child-cwd" ] || log_error "Missing artifact executed a child"
  grep -q 'Cargo did not report' "$fixture/stderr" || log_error "Missing artifact failure was not explained"
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

  tmpdir="$DIR/.wrapper-attestation-$$"
  mkdir "$tmpdir"
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
    sum=$(sha256sum "$(cat "$DOTFILES_ROOT/download-path")" | awk '{print $1}')
    printf '%s  dotfiles-linux-x86_64\n' "$sum" > "$out"
    ;;
  */releases/download/v9.9.9/dotfiles-linux-x86_64)
    mkdir -p "$(dirname "$out")"
    printf '%s\n' "$out" > "$DOTFILES_ROOT/download-path"
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
  for command_name in awk cat chmod dirname mkdir mv readlink rm rmdir sha256sum uname; do
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

test_wrapper_failed_bootstrap_preserves_cached_binary()
{(
  log_stage "Testing failed bootstrap leaves a concurrently installed binary intact"
  fixture="$DIR/.wrapper-staging-$$"
  mkdir "$fixture"
  trap 'rm -rf "$fixture"' EXIT
  mkdir "$fixture/fake-bin"
  cp "$DIR/dotfiles.sh" "$fixture/dotfiles.sh"
  cat > "$fixture/fake-bin/curl" <<'EOF'
#!/bin/sh
set -eu
out=""
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) shift; out=$1 ;;
    http*) url=$1 ;;
  esac
  shift
done
case "$url" in
  */releases/latest) printf '{"tag_name":"v9.9.9"}\n' ;;
  */checksums.sha256)
    if [ "$WRAPPER_FAILURE" = checksum ]; then
      printf 'invalid  dotfiles-linux-x86_64\n' > "$out"
    else
      sum=$(sha256sum "$(cat "$DOTFILES_ROOT/download-path")" | awk '{print $1}')
      printf '%s  dotfiles-linux-x86_64\n' "$sum" > "$out"
    fi
    ;;
  */dotfiles-linux-x86_64)
    printf '#!/bin/sh\nexit 99\n' > "$out"
    printf '%s\n' "$out" > "$DOTFILES_ROOT/download-path"
    # Another bootstrap finished after this one observed a missing cache.
    printf '#!/bin/sh\nexit 0\n' > "$DOTFILES_ROOT/bin/dotfiles"
    chmod +x "$DOTFILES_ROOT/bin/dotfiles"
    [ "$WRAPPER_FAILURE" != download ]
    ;;
  *) exit 1 ;;
esac
EOF
  printf '#!/bin/sh\nexit 1\n' > "$fixture/fake-bin/gh"
  printf '#!/bin/sh\nprintf "x86_64\\n"\n' > "$fixture/fake-bin/uname"
  chmod +x "$fixture/fake-bin/"*
  expected=$(printf '#!/bin/sh\nexit 0\n')
  for failure in download checksum attestation; do
    if PATH="$fixture/fake-bin:$PATH" WRAPPER_FAILURE="$failure" DOTFILES_SKIP_ATTESTATION=0 \
      "$fixture/dotfiles.sh" --version > "$fixture/stdout" 2> "$fixture/stderr"; then
      log_error "$failure failure was accepted"
    fi
    [ -x "$fixture/bin/dotfiles" ] || log_error "$failure removed the concurrent binary"
    [ "$(cat "$fixture/bin/dotfiles")" = "$expected" ] ||
      log_error "$failure modified the concurrent binary"
    [ "$(find "$fixture/bin" -mindepth 1 -maxdepth 1 | wc -l)" -eq 1 ] ||
      log_error "$failure leaked staging files"
    rm "$fixture/bin/dotfiles"
  done
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
    if [ "$#" -gt 0 ]; then
      for test_name in "$@"; do
        "$test_name"
      done
      exit 0
    fi
    test_wrapper_build_mode
    test_wrapper_uses_local_binary
    test_wrapper_forwarded_args
    test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args
    test_wrapper_release_metadata_formats
    test_wrapper_failed_bootstrap_preserves_cached_binary
    test_wrapper_build_mode_consumes_build_flag_and_forwards_cli_args
    test_wrapper_forwards_advanced_flags
    test_wrapper_preserves_runtime_context
    test_wrapper_uses_cargo_artifact
    test_wrapper_chmod_after_checksum
    test_wrapper_attestation_verification
    test_wrapper_release_pinned_urls
    echo "All shell wrapper tests passed"
    ;;
esac
