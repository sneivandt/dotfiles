#!/bin/sh
# shellcheck shell=sh

test_wrapper_bootstrap_downloads_verified_binary_and_forwards_args()
{(
  DOTFILES_RELEASE_REPO=fixture/releases
  export DOTFILES_RELEASE_REPO
  log_stage "Testing real wrapper bootstrap download, checksum, and forwarding"

  tmpdir="$DIR/.wrapper-bootstrap-$$${WRAPPER_BOOTSTRAP_SUFFIX:-}"
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
    cached="$DOTFILES_ROOT/bin/dotfiles"
    if [ "${WRAPPER_EMPTY_CACHE:-0}" = 1 ]; then
      [ -f "$cached" ] && [ ! -s "$cached" ]
    else
      [ ! -e "$cached" ]
    fi || {
      echo "bootstrap published its binary before verification" >&2
      exit 1
    }
    sum=$(sha256sum < "$(cat "$DOTFILES_ROOT/download-path")" | awk '{print $1}')
    printf '%s  dotfiles-linux-x86_64\n' "$sum" > "$out"
    ;;
  */releases/download/v9.9.9/dotfiles-linux-x86_64)
    mkdir -p "$(dirname "$out")"
    printf '%s\n' "$out" > "$DOTFILES_ROOT/download-path"
    cat > "$out" <<'BIN'
#!/bin/sh
if [ "$1" = --version ]; then
  printf 'dotfiles v9.9.9\n'
  exit 0
fi
printf '%s\n' "$DOTFILES_ROOT" > "$DOTFILES_ROOT/root.txt"
printf '%s\n' "$DOTFILES_WRAPPER" > "$DOTFILES_ROOT/wrapper.txt"
printf '%s\n' "$@" > "$DOTFILES_ROOT/args.txt"
printf '%s\n' '{"type":"fixture"}'
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
    for command_name in awk cat chmod dirname flock mkdir mktemp mv readlink rm rmdir sha256sum uname grep; do
      ln -s "$(command -v "$command_name")" "$tmpdir/fake-bin/$command_name"
    done
    wrapper_path="$tmpdir/fake-bin"
  fi

  if [ "${WRAPPER_EMPTY_CACHE:-0}" = 1 ]; then
    mkdir "$tmpdir/bin"
    : > "$tmpdir/bin/dotfiles"
    chmod +x "$tmpdir/bin/dotfiles"
  fi
  status=0
  if [ "${WRAPPER_BLOCK_PUBLISH:-0}" = 1 ]; then
    mkdir -p "$tmpdir/bin"
    (
      flock -xn 8 || exit 2
      PATH="$wrapper_path" DOTFILES_SKIP_ATTESTATION=1 \
        "$tmpdir/dotfiles.sh" update -p desktop -n
    ) 8<>"$tmpdir/bin/.dotfiles.update-state" > "$tmpdir/stdout" 2> "$tmpdir/stderr" || status=$?
    [ "$status" -eq 1 ] || log_error "Bootstrap published while the update lock was held"
    grep -q 'CLI update in progress' "$tmpdir/stderr" || log_error "Missing publication lock error"
    [ ! -e "$tmpdir/bin/dotfiles" ] || log_error "Locked bootstrap installed a binary"
    return 0
  fi
  PATH="$wrapper_path" DOTFILES_SKIP_ATTESTATION=1 \
    "$tmpdir/dotfiles.sh" update -p desktop -n > "$tmpdir/stdout" 2> "$tmpdir/stderr" || status=$?
  if [ "${WRAPPER_EXPECT_RELEASE_ERROR:-0}" = 1 ]; then
    [ "$status" -ne 0 ] || log_error "Wrapper accepted an unsuccessful release lookup"
    [ ! -e "$tmpdir/download-path" ] || log_error "Failed release lookup downloaded an asset"
    [ ! -e "$tmpdir/args.txt" ] || log_error "Failed release lookup executed a child"
    grep -q 'Failed to resolve latest release tag' "$tmpdir/stderr" ||
      log_error "Failed release lookup was not explained"
    return 0
  fi
  [ "$status" -eq 0 ] || log_error "Bootstrap failed: $(cat "$tmpdir/stderr")"
  [ "$(cat "$tmpdir/stdout")" = '{"type":"fixture"}' ] ||
    log_error "Bootstrap mixed progress into child stdout"
  grep -q '^Bootstrap ·' "$tmpdir/stderr" || log_error "Bootstrap start was missing from stderr"
  grep -q '^Downloaded ·' "$tmpdir/stderr" || log_error "Bootstrap completion was missing from stderr"

  expected=$(cat <<'EOF'
update
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

test_wrapper_recovers_truncated_cli()
{(
  log_stage "Testing recovery from an empty executable CLI"
  fixture=$(mktemp -d)
  trap 'rm -rf "$fixture"' EXIT
  cp "$DIR/dotfiles.sh" "$fixture/dotfiles.sh"
  mkdir "$fixture/bin"
  installed="$fixture/bin/dotfiles"
  backup="$fixture/bin/.dotfiles.old"
  journal="$fixture/bin/.dotfiles.update-state"

  : > "$installed"
  : > "$backup"
  chmod +x "$installed" "$backup"
  printf x > "$journal"
  if "$fixture/dotfiles.sh" --version > "$fixture/stdout" 2> "$fixture/stderr"; then
    log_error "Recovery accepted an empty executable"
  fi
  [ -s "$journal" ] && [ ! -s "$installed" ] && [ ! -s "$backup" ] ||
    log_error "Failed recovery modified the journal or either image"
  grep -q 'Neither the installed CLI nor its backup starts' "$fixture/stderr" ||
    log_error "Failed recovery was not explained"

  printf '#!/bin/sh\nprintf "dotfiles v9.9.9\\n"\n' > "$backup"
  [ "$("$fixture/dotfiles.sh" --version 2> "$fixture/stderr")" = 'dotfiles v9.9.9' ] ||
    log_error "Recovery did not run the restored CLI"
  [ ! -e "$backup" ] && [ ! -s "$journal" ] ||
    log_error "Successful recovery did not restore the previous image and clear the journal"
  grep -q '^Restored previous CLI' "$fixture/stderr" ||
    log_error "Successful recovery was not reported"

  rm "$installed" "$journal"
  : > "$backup"
  chmod +x "$backup"
  if "$fixture/dotfiles.sh" --version > "$fixture/stdout" 2> "$fixture/stderr"; then
    log_error "Legacy recovery accepted an empty backup"
  fi
  [ -f "$backup" ] && [ ! -e "$installed" ] ||
    log_error "Failed legacy recovery modified either image"

  cat > "$installed" <<'EOF'
#!/bin/sh
if [ -n "${DOTFILES_COMPLETE:-}" ]; then
  printf 'complete:%s\n' "$DOTFILES_COMPLETE"
  exit 0
fi
[ "${1:-}" = --version ] && printf 'dotfiles v9.9.9\n'
EOF
  cp "$installed" "$backup"
  chmod +x "$installed" "$backup"
  printf x > "$journal"
  completion_out=$(DOTFILES_COMPLETE=zsh "$fixture/dotfiles.sh" 2> "$fixture/stderr") ||
    log_error "Journal recovery rejected a healthy CLI while DOTFILES_COMPLETE was set: $(cat "$fixture/stderr")"
  [ "$completion_out" = 'complete:zsh' ] ||
    log_error "Wrapper stripped DOTFILES_COMPLETE from dispatched CLI: $completion_out"
  [ ! -s "$journal" ] && [ -f "$backup" ] ||
    log_error "Healthy CLI recovery with DOTFILES_COMPLETE did not clear the journal cleanly"
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

test_wrapper_failed_bootstrap_preserves_cached_binary()
{(
  DOTFILES_RELEASE_REPO=fixture/releases
  export DOTFILES_RELEASE_REPO
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
    printf '#!/bin/sh\nprintf "dotfiles v9.9.9\\n"\n' > "$out"
    printf '%s\n' "$out" > "$DOTFILES_ROOT/download-path"
    # Another bootstrap finished after this one observed a missing cache.
    printf '#!/bin/sh\nexit 0\n' > "$DOTFILES_ROOT/bin/dotfiles"
    chmod +x "$DOTFILES_ROOT/bin/dotfiles"
    [ "$WRAPPER_FAILURE" != download ]
    ;;
  *) exit 1 ;;
esac
EOF
  cat > "$fixture/fake-bin/gh" <<'EOF'
#!/bin/sh
[ "$WRAPPER_FAILURE" = publication ]
EOF
  printf '#!/bin/sh\nprintf "x86_64\\n"\n' > "$fixture/fake-bin/uname"
  chmod +x "$fixture/fake-bin/"*
  expected=$(printf '#!/bin/sh\nexit 0\n')
  for failure in download checksum attestation publication; do
    if PATH="$fixture/fake-bin:$PATH" WRAPPER_FAILURE="$failure" DOTFILES_SKIP_ATTESTATION=0 \
      "$fixture/dotfiles.sh" --version > "$fixture/stdout" 2> "$fixture/stderr"; then
      log_error "$failure failure was accepted"
    fi
    [ -x "$fixture/bin/dotfiles" ] || log_error "$failure removed the concurrent binary"
    [ "$(cat "$fixture/bin/dotfiles")" = "$expected" ] ||
      log_error "$failure modified the concurrent binary"
    [ "$(find "$fixture/bin" -mindepth 1 -maxdepth 1 ! -name .dotfiles.update-state | wc -l)" -eq 1 ] ||
      log_error "$failure leaked staging files"
    if [ "$failure" = publication ]; then
      grep -q 'changed during bootstrap' "$fixture/stderr" || log_error "Wrong publication failure"
      "$fixture/dotfiles.sh" --version || log_error "Retry did not use the concurrent binary"
    fi
    rm "$fixture/bin/dotfiles"
  done
)}

test_wrapper_release_pinned_urls()
{(
  log_stage "Testing wrapper resolves release tag and uses pinned URLs for binary and checksum"

  wrapper="$DIR/dotfiles.sh"
  content=$(cat "$wrapper")

  if echo "$content" | grep -q "resolve_release_tag" && \
     echo "$content" | grep -Fq "releases/download/\$tag" && \
     ! echo "$content" | grep -q 'releases/latest/download'; then
    log_verbose "✓ Wrapper resolves release tag and uses pinned URLs for binary and checksum"
  else
    printf "%sERROR: Wrapper does not use version-pinned URLs for bootstrap downloads%s\n" "${RED}" "${NC}" >&2
    return 1
  fi
)}

test_wrapper_release_repository()
{(
  log_stage "Testing release repository discovery and overrides without network access"
  fixture=$(mktemp -d)
  trap 'rm -rf "$fixture"' EXIT
  mkdir -p "$fixture/repo space" "$fixture/fake-bin"
  cp "$DIR/dotfiles.sh" "$fixture/repo space/dotfiles.sh"
  git init -q "$fixture/repo space"
  git -C "$fixture/repo space" remote add origin https://github.com/fork-owner/custom-name.git
  cat > "$fixture/fake-bin/curl" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" > "$DOTFILES_ROOT/request"
exit 22
EOF
  chmod +x "$fixture/fake-bin/curl"
  PATH="$fixture/fake-bin:$PATH"
  export PATH
  unset DOTFILES_RELEASE_REPO
  for remote in https://github.com/fork-owner/custom-name.git git@github.com:fork-owner/custom-name.git ssh://git@github.com/fork-owner/custom-name.git; do
    git -C "$fixture/repo space" remote set-url origin "$remote"
    if "$fixture/repo space/dotfiles.sh" --version > "$fixture/stdout" 2> "$fixture/stderr"; then
      log_error "Fixture unexpectedly downloaded a binary"
    fi
    grep -Fxq 'https://api.github.com/repos/fork-owner/custom-name/releases/latest' "$fixture/repo space/request" ||
      log_error "Wrong repository discovered from $remote"
  done
  git -C "$fixture/repo space" remote set-url origin https://example.com/other/repo
  for override in publisher/releases '' owner/repo/extra '../repo' 'owner/repo?query'; do
    rm -f "$fixture/repo space/request"
    if DOTFILES_RELEASE_REPO="$override" "$fixture/repo space/dotfiles.sh" --version > "$fixture/stdout" 2> "$fixture/stderr"; then
      log_error "Fixture unexpectedly succeeded"
    fi
    if [ "$override" = publisher/releases ]; then
      grep -Fxq 'https://api.github.com/repos/publisher/releases/releases/latest' "$fixture/repo space/request" ||
        log_error "Override was not used"
    else
      [ ! -e "$fixture/repo space/request" ] || log_error "Invalid override reached network tool"
      grep -q DOTFILES_RELEASE_REPO "$fixture/stderr" || log_error "Missing actionable repository error"
    fi
  done
  rm -f "$fixture/repo space/request"
  if "$fixture/repo space/dotfiles.sh" --version > "$fixture/stdout" 2> "$fixture/stderr"; then
    log_error "Non-GitHub origin unexpectedly accepted"
  fi
  [ ! -e "$fixture/repo space/request" ] || log_error "Non-GitHub origin reached network tool"
  git -C "$fixture/repo space" remote remove origin
  if "$fixture/repo space/dotfiles.sh" --version > "$fixture/stdout" 2> "$fixture/stderr"; then
    log_error "Missing origin unexpectedly accepted"
  fi
  [ ! -e "$fixture/repo space/request" ] || log_error "Missing origin reached network tool"
)}

test_wrapper_api_headers_and_error_diagnostics()
{(
  log_stage "Testing wrapper GitHub API headers, stdin token passing, and error diagnostics"
  fixture=$(mktemp -d)
  trap 'rm -rf "$fixture"' EXIT
  mkdir -p "$fixture/fake-bin"
  cp "$DIR/dotfiles.sh" "$fixture/dotfiles.sh"
  cat > "$fixture/fake-bin/curl" <<'EOF'
#!/bin/sh
printf '%s\n' "$@" > "$DOTFILES_ROOT/argv"
if [ ! -t 0 ]; then
  cat > "$DOTFILES_ROOT/stdin"
fi
echo "curl: (22) The requested URL returned error: 403" >&2
exit 22
EOF
  chmod +x "$fixture/fake-bin/curl"

  if PATH="$fixture/fake-bin:$PATH" DOTFILES_RELEASE_REPO=fixture/releases \
    GH_TOKEN=fixture-tok GITHUB_TOKEN='' \
    "$fixture/dotfiles.sh" --version > "$fixture/stdout" 2> "$fixture/stderr"; then
    log_error "Failed API call unexpectedly succeeded"
  fi
  grep -Fxq 'Accept: application/vnd.github+json' "$fixture/argv" ||
    log_error "Wrapper did not send Accept header on GitHub API request"
  if grep -Fq 'fixture-tok' "$fixture/argv"; then
    log_error "Wrapper exposed GH_TOKEN in curl argv"
  fi
  grep -Fxq 'header = "Authorization: Bearer fixture-tok"' "$fixture/stdin" ||
    log_error "Wrapper did not pass Authorization header via curl -K stdin"
  grep -Fq '403' "$fixture/stderr" ||
    log_error "Wrapper discarded curl stderr diagnostic on failure"
)}
