#!/bin/sh
# shellcheck shell=sh

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

  cd "$DIR" || exit 1

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

  PATH="$tmpdir/fake-bin:$PATH" "$tmpdir/dotfiles.sh" --build update -p desktop -n -v

  expected=$(cat <<EOF
update
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
    "$tmpdir/dotfiles.sh" --build update --skip symlinks --only packages --no-parallel

  expected=$(cat <<'EOF'
update
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
: > "$DOTFILES_ROOT/child-args"
for argument do
  printf '%s\0' "$argument" >> "$DOTFILES_ROOT/child-args"
done
printf 'child stdout\n'
printf 'child stderr\n' >&2
exit 7
EOF
  cp "$tmpdir/bin/dotfiles" "$tmpdir/cli/target/dev-opt/dotfiles"
  chmod +x "$tmpdir/fake-bin/cargo" "$tmpdir/bin/dotfiles" "$tmpdir/cli/target/dev-opt/dotfiles"
  PATH="$tmpdir/fake-bin:$PATH"
  DOTFILES_REEXEC_GUARD=1
  export PATH DOTFILES_REEXEC_GUARD
  cd "$tmpdir/caller dir" || exit 1
  ln -s ../dotfiles.sh 'wrapper link'
  for mode in cached build; do
    for arguments in empty complex; do
      set --
      if [ "$arguments" = complex ]; then
        set -- update --dry-run --root . --overlay "../overlay dir" "" --BUILD -- \
          "literal value" --build '*?[literal]' 'quote"and apostrophe'"'" 'semi;colon' \
          'back\slash' "$(printf 'line\nbreak\tvalue')" "" ""
      fi
      expected_argc=$#
      : > "$tmpdir/expected-args"
      for argument do
        printf '%s\0' "$argument" >> "$tmpdir/expected-args"
      done
      [ "$mode" != build ] || set -- --build --build "$@"
      rm -f "$tmpdir/child-cwd" "$tmpdir/child-args"
      status=0
      './wrapper link' "$@" > "$tmpdir/stdout" 2> "$tmpdir/stderr" || status=$?
      [ "$status" -eq 7 ] || log_error "$mode/$arguments wrapper lost the child exit code"
      [ "$(cat "$tmpdir/child-cwd")" = "$PWD" ] || log_error "$mode wrapper changed the child cwd"
      [ "$(cat "$tmpdir/child-context")" = "$(printf '%s\n' "$tmpdir" sh 1)" ] || log_error "$mode wrapper changed runtime context"
      [ "$(cat "$tmpdir/child-argc")" -eq "$expected_argc" ] || log_error "$mode wrapper changed the argument count"
      cmp -s "$tmpdir/child-args" "$tmpdir/expected-args" || log_error "$mode wrapper changed argument bytes or boundaries"
      [ "$(cat "$tmpdir/stdout")" = 'child stdout' ] || log_error "$mode wrapper changed stdout"
      [ "$(cat "$tmpdir/stderr")" = 'child stderr' ] || log_error "$mode wrapper changed stderr"
    done
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
if [ "${WRAPPER_ARTIFACT_MODE:-valid}" != missing ]; then
  escaped=$(printf '%s' "$executable" | sed 's/\\/\\\\/g; s/"/\\"/g; s,/,\\u002f,g; s/ /\\u0020/g')
  printf '{"reason":"compiler-artifact","target":{"name":"another-binary"},"executable":"not-the-dotfiles-artifact"}\n'
  printf '{"reason":"compiler-artifact","target":{"name":"dotfiles"},"executable":"%s"}\n' "$escaped"
  if [ "${WRAPPER_ARTIFACT_MODE:-valid}" = duplicate ]; then
    printf '{"reason":"compiler-artifact","target":{"name":"dotfiles"},"executable":"%s"}\n' "$escaped"
  fi
fi
printf '{"reason":"build-finished","success":true}\n'
EOF
  chmod +x "$fixture/fake-bin/cargo" "$fixture/cli/target/dev-opt/dotfiles"
  PATH="$fixture/fake-bin:$PATH"
  export PATH
  cd "$fixture/caller dir" || exit 1
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
  for artifact_mode in missing duplicate; do
    if WRAPPER_ARTIFACT_MODE="$artifact_mode" "$fixture/dotfiles.sh" --build --version > "$fixture/stdout" 2> "$fixture/stderr"; then
      log_error "$artifact_mode Cargo artifact did not fail"
    fi
    [ ! -e "$fixture/child-cwd" ] || log_error "$artifact_mode artifact executed a child"
    [ ! -s "$fixture/stdout" ] || log_error "$artifact_mode artifact leaked Cargo output"
    grep -q 'Cargo did not report a unique dotfiles executable' "$fixture/stderr" ||
      log_error "$artifact_mode artifact failure was not explained"
  done
)}

