#!/bin/sh
# shellcheck shell=sh

test_github_ssh_allowlist()
{(
  log_stage "Checking GitHub SSH exceptions preserve scanning of the rest of the line"
  fixture=$(mktemp -d)
  trap 'rm -rf "$fixture"' EXIT
  git init -q "$fixture"
  mkdir "$fixture/hooks"
  cp "$DIR/hooks/check-sensitive.sh" "$DIR/hooks/sensitive-patterns.ini" \
    "$DIR/hooks/sensitive-allowlist.ini" "$fixture/hooks/"
  cd "$fixture" || exit 1
  for remote in \
    git@github.com:fixture/repo.git \
    ssh://git@github.com/fixture/repo.git \
    git@ssh.dev.azure.com:v3/fixture/project/repo \
    ssh://git@ssh.dev.azure.com/v3/fixture/project/repo; do
    printf '%s\n' "$remote" > remote.txt
    git add remote.txt
    sh hooks/check-sensitive.sh > "$fixture/output" 2>&1 || log_error "Public Git SSH URL was rejected: $remote"
    printf '%s token = ghp_%s\n' "$remote" 'abcdefghijklmnopqrstuvwxyzabcdefghijklmn' > remote.txt
    git add remote.txt
    if sh hooks/check-sensitive.sh > "$fixture/output" 2>&1; then
      log_error "Git SSH exception hid a token on the same line: $remote"
    fi
  done
  for lookalike in github.com.example ssh.dev.azure.com.example; do
    printf 'git@%s\n' "$lookalike" > remote.txt
    git add remote.txt
    if sh hooks/check-sensitive.sh > "$fixture/output" 2>&1; then
      log_error "Git SSH exception accepted a lookalike email domain: $lookalike"
    fi
  done
)}

test_shell_sha256_case_allowlist()
{(
  log_stage "Checking shell SHA-256 case arm exceptions preserve scanning of appended secrets"
  fixture=$(mktemp -d)
  trap 'rm -rf "$fixture"' EXIT
  git init -q "$fixture"
  mkdir "$fixture/hooks"
  cp "$DIR/hooks/check-sensitive.sh" "$DIR/hooks/sensitive-patterns.ini" \
    "$DIR/hooks/sensitive-allowlist.ini" "$fixture/hooks/"
  cd "$fixture" || exit 1
  checksum=$(printf '%064d' 0)
  printf '  %s | \\\n  %s)\n' "$checksum" "$checksum" > shim.sh
  git add shim.sh
  sh hooks/check-sensitive.sh > "$fixture/output" 2>&1 || log_error "Shell SHA-256 case digest was rejected"
  printf '  %s) token = ghp_%s\n' "$checksum" 'abcdefghijklmnopqrstuvwxyzabcdefghijklmn' > shim.sh
  git add shim.sh
  if sh hooks/check-sensitive.sh > "$fixture/output" 2>&1; then
    log_error "Shell SHA-256 case digest exception hid a token on the same line"
  fi
)}

test_pkgbuild_checksum_allowlist()
{(
  log_stage "Checking PKGBUILD checksum exceptions preserve scanning of appended secrets"
  fixture=$(mktemp -d)
  trap 'rm -rf "$fixture"' EXIT
  git init -q "$fixture"
  mkdir "$fixture/hooks"
  cp "$DIR/hooks/check-sensitive.sh" "$DIR/hooks/sensitive-patterns.ini" \
    "$DIR/hooks/sensitive-allowlist.ini" "$fixture/hooks/"
  cd "$fixture" || exit 1
  checksum=$(printf '%064d' 0)
  for variable in sha256sums sha256sums_x86_64; do
    printf "%s=('%s' '%s')\n" "$variable" "$checksum" "$checksum" > PKGBUILD
    git add PKGBUILD
    sh hooks/check-sensitive.sh > "$fixture/output" 2>&1 || log_error "PKGBUILD checksum was rejected"
    printf "%s=('%s'); token = ghp_%s\n" "$variable" "$checksum" \
      'abcdefghijklmnopqrstuvwxyzabcdefghijklmn' > PKGBUILD
    git add PKGBUILD
    if sh hooks/check-sensitive.sh > "$fixture/output" 2>&1; then
      log_error "PKGBUILD checksum exception hid a token on the same line"
    fi
  done
)}

test_sensitive_posix_patterns()
{(
  log_stage "Checking portable ERE rules and case-insensitive safe-span redaction"
  fixture=$(mktemp -d)
  trap 'rm -rf "$fixture"' EXIT
  mkdir "$fixture/repo" "$fixture/bin"
  cd "$fixture/repo" || exit 1
  init_fixture_repository
  mkdir hooks
  cp "$DIR/hooks/check-sensitive.sh" "$DIR/hooks/sensitive-patterns.ini" \
    "$DIR/hooks/sensitive-allowlist.ini" hooks/
  real_grep=$(command -v grep)
  real_sed=$(command -v sed)
  cat > "$fixture/bin/grep" <<'EOF'
#!/bin/sh
for arg do
  case "$arg" in *'\s'*|*'\b'*) exit 2 ;; esac
done
exec "$STRICT_GREP" "$@"
EOF
  cat > "$fixture/bin/sed" <<'EOF'
#!/bin/sh
for arg do
  case "$arg" in *gI*) exit 2 ;; esac
done
exec "$STRICT_SED" "$@"
EOF
  chmod +x "$fixture/bin/grep" "$fixture/bin/sed"
  STRICT_GREP="$real_grep" STRICT_SED="$real_sed"
  PATH="$fixture/bin:$PATH"
  export STRICT_GREP STRICT_SED PATH
  printf 'uses: OWNER/REPO@%040d\n' 0 > fixture.txt
  git add fixture.txt
  sh hooks/check-sensitive.sh > "$fixture/output" 2>&1 ||
    log_error "Portable scanner rejected a mixed-case allowlisted span"
  for field in client_secret key; do
    length=24
    [ "$field" != key ] || length=40
    value=$(printf "%${length}s" x | tr ' ' x)
    printf 'uses: OWNER/REPO@%040d; %s \t= \t"%s"\n' 0 "$field" "$value" > fixture.txt
    git add fixture.txt
    if sh hooks/check-sensitive.sh > "$fixture/output" 2>&1; then
      log_error "Portable $field rule missed a secret beside an allowlisted span"
    fi
    grep -q 'Potential sensitive information' "$fixture/output" ||
      log_error "Portable scanner failed instead of detecting $field"
  done
)}

