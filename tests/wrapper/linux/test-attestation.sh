#!/bin/sh
# shellcheck shell=sh

test_wrapper_attestation_verification()
{(
  DOTFILES_RELEASE_REPO=fixture/releases
  export DOTFILES_RELEASE_REPO
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
    cat > "$out" <<'BIN'
#!/bin/sh
printf '%s\n' "$GH_PROMPT_DISABLED" "$DBUS_SESSION_BUS_ADDRESS" > "$DOTFILES_ROOT/child-env"
[ "${WRAPPER_BAD_BINARY:-0}" = 0 ] || exit 42
printf 'dotfiles v9.9.9\n'
BIN
    ;;
  *)
    echo "unexpected URL: $url" >&2
    exit 1
    ;;
esac
EOF
  chmod +x "$tmpdir/fake-bin/curl"

  # Record the subprocess environment even when verification fails.
  cat > "$tmpdir/fake-bin/gh" <<'EOF'
#!/bin/sh
printf '%s\n' "$GH_PROMPT_DISABLED" "$DBUS_SESSION_BUS_ADDRESS" > "$DOTFILES_ROOT/gh-env"
exit "${WRAPPER_GH_EXIT:-1}"
EOF
  chmod +x "$tmpdir/fake-bin/gh"

  # Required (default): unverified provenance is fatal and removes the download.
  rm -rf "${tmpdir:?}/bin"
  if PATH="$tmpdir/fake-bin:$PATH" DOTFILES_SKIP_ATTESTATION=0 \
    GH_PROMPT_DISABLED=caller DBUS_SESSION_BUS_ADDRESS=caller \
    "$tmpdir/dotfiles.sh" --version > "$tmpdir/stdout" 2> "$tmpdir/stderr"; then
    printf "%sERROR: Default policy accepted an unverified binary%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  if [ -e "$tmpdir/bin/dotfiles" ]; then
    printf "%sERROR: Unverified binary was not removed by default%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  expected=$(printf '1\ndisabled:')
  [ "$(cat "$tmpdir/gh-env")" = "$expected" ] ||
    log_error "Verification did not disable terminal and desktop keyring prompts"
  grep -q 'GH_TOKEN/GITHUB_TOKEN' "$tmpdir/stderr" ||
    log_error "Verification failure did not explain non-interactive credentials"
  log_verbose "✓ Default policy rejects unverified downloads"

  if PATH="$tmpdir/fake-bin:$PATH" DOTFILES_SKIP_ATTESTATION=0 WRAPPER_GH_EXIT=0 \
    WRAPPER_BAD_BINARY=1 GH_PROMPT_DISABLED=caller DBUS_SESSION_BUS_ADDRESS=caller \
    "$tmpdir/dotfiles.sh" --version > "$tmpdir/stdout" 2> "$tmpdir/stderr"; then
    log_error "Bootstrap accepted a verified executable that failed to start"
  fi
  [ ! -e "$tmpdir/bin/dotfiles" ] || log_error "Failed smoke check published an executable"
  grep -q 'Downloaded CLI did not start' "$tmpdir/stderr" || log_error "Failed smoke check was not explained"

  PATH="$tmpdir/fake-bin:$PATH" DOTFILES_SKIP_ATTESTATION=0 WRAPPER_GH_EXIT=0 \
    GH_PROMPT_DISABLED=caller DBUS_SESSION_BUS_ADDRESS=caller \
    "$tmpdir/dotfiles.sh" --version >/dev/null 2>&1
  expected=$(printf 'caller\ncaller')
  [ "$(cat "$tmpdir/child-env")" = "$expected" ] ||
    log_error "Verification changed the executed binary's environment"

  # Skip: verification is bypassed entirely.
  rm -rf "${tmpdir:?}/bin"
  rm -f "$tmpdir/gh-env"
  PATH="$tmpdir/fake-bin:$PATH" DOTFILES_SKIP_ATTESTATION=1 \
    GH_PROMPT_DISABLED=caller DBUS_SESSION_BUS_ADDRESS=caller \
    "$tmpdir/dotfiles.sh" --version >/dev/null 2>&1
  if [ ! -x "$tmpdir/bin/dotfiles" ]; then
    printf "%sERROR: DOTFILES_SKIP_ATTESTATION=1 did not bypass verification%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
  [ ! -e "$tmpdir/gh-env" ] || log_error "Skip policy invoked gh"
  log_verbose "✓ DOTFILES_SKIP_ATTESTATION=1 bypasses provenance verification"

  # Missing gh: fail closed, unless provenance is explicitly bypassed.
  rm -rf "${tmpdir:?}/bin"
  rm -f "$tmpdir/fake-bin/gh"
  for command_name in awk cat chmod dirname flock mkdir mktemp mv readlink rm rmdir sha256sum uname grep; do
    ln -s "$(command -v "$command_name")" "$tmpdir/fake-bin/$command_name"
  done
  if PATH="$tmpdir/fake-bin" DOTFILES_SKIP_ATTESTATION=0 \
    "$tmpdir/dotfiles.sh" --version > "$tmpdir/stdout" 2> "$tmpdir/stderr"; then
    log_error "Missing gh accepted an unattested binary"
  fi
  [ ! -e "$tmpdir/bin/dotfiles" ] || log_error "Missing gh published an unattested binary"
  grep -q 'ERROR: gh not found' "$tmpdir/stderr" || log_error "Missing gh was not explained"
  PATH="$tmpdir/fake-bin" DOTFILES_SKIP_ATTESTATION=1 \
    "$tmpdir/dotfiles.sh" --version >/dev/null 2>&1
  [ -x "$tmpdir/bin/dotfiles" ] || log_error "Explicit bypass did not work without gh"
)}

