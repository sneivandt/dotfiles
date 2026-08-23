#!/bin/sh
#
# CI test for pre-commit hook - verifies sensitive pattern detection
#
# This test validates that the pre-commit hook correctly:
# - Blocks commits containing sensitive patterns
# - Allows clean commits without sensitive data
# - Reports the correct pattern matches

set -o errexit
set -o nounset

# This suite creates real commits in the current repository to exercise the
# pre-commit hook. Refuse to run against a dirty checkout so in-progress work
# can never be swept into a test commit if a detection case fails to block.
# CI runs this on a fresh checkout, so the guard is inert there.
if [ "${DOTFILES_HOOK_TEST_FORCE:-0}" != "1" ]; then
  if ! git diff --cached --quiet || ! git diff --quiet; then
    printf 'ERROR: refusing to run against a repository with uncommitted changes.\n' >&2
    printf 'This suite creates commits in the current repository, so a detection\n' >&2
    printf 'failure would commit your staged work.\n\n' >&2
    printf 'Run it on a clean checkout, or in a scratch repository:\n' >&2
    printf '  mkdir -p /tmp/hooktest/hooks && cp -a hooks/. /tmp/hooktest/hooks/\n' >&2
    printf '  cd /tmp/hooktest && git init -q\n' >&2
    printf '  ln -sf /tmp/hooktest/hooks/pre-commit .git/hooks/pre-commit\n' >&2
    printf '  DIR=<repo> %s\n\n' "$0" >&2
    printf 'Set DOTFILES_HOOK_TEST_FORCE=1 to override.\n' >&2
    exit 1
  fi
fi

# Test counter
tests_passed=0
tests_failed=0

# Test helper functions
pass() {
  printf "✓ %s\n" "$1"
  tests_passed=$((tests_passed + 1))
}

fail() {
  printf "✗ %s\n" "$1"
  tests_failed=$((tests_failed + 1))
}

# Test that hook blocks a commit with sensitive content
test_hook_blocks() {
  pattern_type="$1"
  sensitive_content="$2"

  # Create a test file with sensitive content
  echo "$sensitive_content" > test-file.txt
  git add test-file.txt

  # Try to commit (should fail) without echoing the detected value.
  result=1
  if commit_output=$(git commit -m "Test commit with $pattern_type" 2>&1); then
    fail "Hook allowed commit with $pattern_type"
  elif ! printf '%s\n' "$commit_output" | grep -q "Potential sensitive information detected"; then
    fail "Hook failed to report $pattern_type"
  elif printf '%s\n' "$commit_output" | grep -Fq -- "$sensitive_content"; then
    fail "Hook printed detected $pattern_type content"
  else
    pass "Hook blocked commit with $pattern_type"
    result=0
  fi

  git reset HEAD test-file.txt > /dev/null 2>&1 || true
  rm -f test-file.txt
  return "$result"
}

test_hook_blocks_staged_file_deleted_from_worktree() {
  printf 'secret_%s: "%s%s%s%s"\n' \
    'key' 'abcdef12345' '67890abcdef' '12345' '67890' > staged-only-secret.txt
  git add staged-only-secret.txt
  rm -f staged-only-secret.txt

  if git commit -m "Test staged-only sensitive file" 2>&1 | grep -q "Potential sensitive information detected"; then
    pass "Hook scanned a staged file deleted from the worktree"
    git reset HEAD staged-only-secret.txt > /dev/null 2>&1 || true
    return 0
  fi

  fail "Hook skipped a staged file deleted from the worktree"
  git reset HEAD staged-only-secret.txt > /dev/null 2>&1 || true
  return 1
}

# Test that hook allows clean commits
test_hook_allows() {
  clean_content="$1"

  # Create a test file with clean content
  echo "$clean_content" > test-file.txt
  git add test-file.txt

  # Try to commit (should succeed)
  if git commit -m "Test clean commit" > /dev/null 2>&1; then
    pass "Hook allowed clean commit"
    git reset --soft HEAD~1 > /dev/null 2>&1 || true
    git reset HEAD test-file.txt > /dev/null 2>&1 || true
    rm -f test-file.txt
    return 0
  else
    fail "Hook incorrectly blocked clean commit"
    git reset HEAD test-file.txt > /dev/null 2>&1 || true
    rm -f test-file.txt
    return 1
  fi
}

test_hook_wiring() {
  repo_root="${DIR:-$(git rev-parse --show-toplevel)}"

  for script in check-sensitive.sh check-rust.sh check-ci-guards.sh; do
    if [ ! -f "$repo_root/hooks/$script" ]; then
      fail "Missing hook helper $script"
      return 1
    fi
    if ! grep -Fq "$script" "$repo_root/hooks/pre-commit"; then
      fail "pre-commit does not call $script"
      return 1
    fi
    if ! sh -n "$repo_root/hooks/$script"; then
      fail "$script has invalid POSIX shell syntax"
      return 1
    fi
  done

  pass "Pre-commit hook references all helper scripts"
}

test_hook_modes() {
  repo="$(mktemp -d)"
  hook_log="$repo/hook.log"

  git init -q "$repo"
  mkdir -p "$repo/hooks"
  cp "${DIR:-$(git rev-parse --show-toplevel)}/hooks/pre-commit" "$repo/hooks/pre-commit"

  for script in check-sensitive.sh check-rust.sh check-ci-guards.sh; do
    cat > "$repo/hooks/$script" <<EOF
#!/bin/sh
printf '%s\n' '$script' >> "\$HOOK_LOG"
EOF
  done

  (
    cd "$repo"
    HOOK_LOG="$hook_log" sh hooks/pre-commit
  )

  expected=$(printf '%s\n' check-sensitive.sh check-rust.sh)
  if [ "$(cat "$hook_log")" = "$expected" ]; then
    pass "Default hook runs sensitive-data and Rust checks"
  else
    fail "Default hook did not run the expected checks"
  fi

  : > "$hook_log"
  (
    cd "$repo"
    HOOK_LOG="$hook_log" DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
  )

  expected=$(printf '%s\n' check-sensitive.sh check-rust.sh check-ci-guards.sh)
  if [ "$(cat "$hook_log")" = "$expected" ]; then
    pass "Full hook mode runs all helper scripts"
  else
    fail "Full hook mode did not run all helper scripts"
  fi

  rm -rf "$repo"
}

test_release_workflow_guards() {
  repo_root="${DIR:-$(git rev-parse --show-toplevel)}"
  repo="$(mktemp -d)"

  git init -q "$repo"
  mkdir -p "$repo/hooks" "$repo/.github/workflows"
  cp "$repo_root/hooks/check-ci-guards.sh" "$repo/hooks/check-ci-guards.sh"
  cp "$repo_root/.github/workflows/release.yml" "$repo/.github/workflows/release.yml"
  git -C "$repo" add .github/workflows/release.yml

  if (
    cd "$repo"
    sh hooks/check-ci-guards.sh >/dev/null 2>&1
  ); then
    pass "Release workflow guards accept the hardened workflow"
  else
    fail "Release workflow guards rejected the hardened workflow"
  fi

  sed '/^[[:space:]]*- name: Verify attestation discoverability[[:space:]]*$/,/^[[:space:]]*- name: Create release[[:space:]]*$/{ /^[[:space:]]*- name: Create release[[:space:]]*$/!d; }' \
    "$repo_root/.github/workflows/release.yml" > "$repo/.github/workflows/release.yml"
  git -C "$repo" add .github/workflows/release.yml

  if (
    cd "$repo"
    sh hooks/check-ci-guards.sh >/dev/null 2>&1
  ); then
    fail "Release workflow guards accepted publication without proving attestation discoverability"
  else
    pass "Release workflow guards require attestation verification before publication"
  fi

  sed '/^[[:space:]]*group:[[:space:]]*release[[:space:]]*$/d' \
    "$repo_root/.github/workflows/release.yml" > "$repo/.github/workflows/release.yml"
  git -C "$repo" add .github/workflows/release.yml
  cp "$repo_root/.github/workflows/release.yml" "$repo/.github/workflows/release.yml"

  if (
    cd "$repo"
    sh hooks/check-ci-guards.sh >/dev/null 2>&1
  ); then
    fail "Release workflow guards read the safe working tree instead of the unsafe index"
  else
    pass "Release workflow guards validate the staged workflow"
  fi

  rm -rf "$repo"
}

test_sensitive_scan_without_paste() {
  repo_root="${DIR:-$(git rev-parse --show-toplevel)}"
  repo="$(mktemp -d)"

  git init -q "$repo"
  mkdir -p "$repo/hooks" "$repo/bin"
  cp "$repo_root/hooks/check-sensitive.sh" "$repo/hooks/check-sensitive.sh"
  cp "$repo_root/hooks/sensitive-patterns.ini" "$repo/hooks/sensitive-patterns.ini"
  cp "$repo_root/hooks/sensitive-allowlist.ini" "$repo/hooks/sensitive-allowlist.ini"

  cat > "$repo/bin/paste" <<'EOF'
#!/bin/sh
exit 127
EOF
  chmod +x "$repo/bin/paste"

  printf '%s\n' 'release = v2026.07.30-1' > "$repo/test-file.txt"
  git -C "$repo" add test-file.txt

  if (
    cd "$repo"
    PATH="$repo/bin:$PATH" sh hooks/check-sensitive.sh >/dev/null 2>&1
  ); then
    pass "Sensitive-data scan does not require paste"
  else
    fail "Sensitive-data scan requires unavailable paste"
  fi

  rm -rf "$repo"
}

test_pre_push_protection() {
  repo_root="${DIR:-$(git rev-parse --show-toplevel)}"
  hook="$repo_root/symlinks/config/git/templates/hooks.local/pre-push"
  repo="$(mktemp -d)"
  sha=1111111111111111111111111111111111111111
  zero=0000000000000000000000000000000000000000

  git init -q "$repo"
  git -C "$repo" config github.user ci-owner
  git -C "$repo" remote add origin https://github.com/ci-owner/repo.git

  if (
    cd "$repo"
    printf "refs/heads/feature %s refs/heads/main %s\n" "$sha" "$zero" |
      sh "$hook" upstream https://github.com/upstream/repo.git >/dev/null 2>&1
  ); then
    fail "Pre-push hook allowed a protected branch on the actual upstream remote"
  else
    pass "Pre-push hook blocks protected branches on the actual remote"
  fi

  if (
    cd "$repo"
    printf "refs/heads/main %s refs/heads/main %s\n" "$sha" "$zero" |
      sh "$hook" origin https://github.com/ci-owner/repo.git >/dev/null 2>&1
  ); then
    pass "Pre-push hook allows protected branches on owned repositories"
  else
    fail "Pre-push hook blocked an owned repository"
  fi

  if (
    cd "$repo"
    printf "refs/heads/feature %s refs/heads/feature %s\n" "$sha" "$zero" |
      sh "$hook" upstream https://github.com/upstream/repo.git >/dev/null 2>&1
  ); then
    pass "Pre-push hook allows non-protected branches"
  else
    fail "Pre-push hook blocked a non-protected branch"
  fi

  rm -rf "$repo"
}

printf "Testing pre-commit hook sensitive pattern detection\n"
printf "==================================================\n\n"

printf "Testing hook wiring...\n"
test_hook_wiring

printf "Testing hook modes...\n"
test_hook_modes

printf "Testing release workflow guards...\n"
test_release_workflow_guards

printf "Testing hook portability...\n"
test_sensitive_scan_without_paste

printf "Testing pre-push protection...\n"
test_pre_push_protection

# Test AWS credentials
printf "Testing AWS patterns...\n"
test_hook_blocks "AWS access key" "aws_access_key_id = AKIAIOSFODNN7EXAMPLE"
test_hook_blocks "AWS key ID format" "export AWS_KEY=AKIAIOSFODNN7EXAMPLE"

# Test GitHub tokens
printf "\nTesting GitHub patterns...\n"
test_hook_blocks "GitHub PAT" "token = ghp_1234567890123456789012345678901234567890"
test_hook_blocks "GitHub token assignment" "GITHUB_TOKEN=gho_abcdefghijklmnopqrstuvwxyz12345678901234"
fine_grained_pat="github_pat_$(printf '%s' 'abcdefghijklmnopqrstuv')"
test_hook_blocks "GitHub fine-grained PAT" "GITHUB_TOKEN=$fine_grained_pat"

# Test API keys
printf "\nTesting API key patterns...\n"
test_hook_blocks "API key" 'apikey = "1234567890abcdef1234567890abcdef"'
test_hook_blocks "Secret key" 'secret_key: "abcdef1234567890abcdef1234567890"'
test_hook_blocks_staged_file_deleted_from_worktree

# Test passwords
printf "\nTesting password patterns...\n"
test_hook_blocks "Password" 'password = "MySecretPass123"'
test_hook_blocks "Passwd" 'passwd: "AnotherPassword456"'

# Test private keys
printf "\nTesting private key patterns...\n"
test_hook_blocks "RSA private key" "-----BEGIN RSA PRIVATE KEY-----"
test_hook_blocks "OpenSSH private key" "-----BEGIN OPENSSH PRIVATE KEY-----"

# Test database connection strings
printf "\nTesting database patterns...\n"
test_hook_blocks "MySQL connection" "mysql://user:password@localhost:3306/db"
test_hook_blocks "PostgreSQL connection" "postgresql://admin:secret@db.example.com/mydb"

# Test Heroku API keys (UUID format with context)
printf "\nTesting Heroku patterns...\n"
test_hook_blocks "Heroku API key" "HEROKU_API_KEY=550e8400-e29b-41d4-a716-446655440000"

# Test Slack tokens
printf "\nTesting Slack patterns...\n"
test_hook_blocks "Slack bot token" "SLACK_TOKEN=xoxb-1111111111111-2222222222222-EXAMPLEEXAMPLEEXAMPLEEXA"

# Test Stripe keys
printf "\nTesting Stripe patterns...\n"
test_hook_blocks "Stripe secret key" "STRIPE_SECRET=sk_test_EXAMPLEKEY1234567890abcd"

# Test Google API keys
printf "\nTesting Google patterns...\n"
test_hook_blocks "Google API key" "GOOGLE_API_KEY=AIzaSyC1234567890abcdefghijklmnopqrstuvw"

# Test GitLab tokens
printf "\nTesting GitLab patterns...\n"
test_hook_blocks "GitLab PAT" "GITLAB_TOKEN=glpat-abcdefghijklmnopqrstuvwxyz"

# Test OAuth secrets
printf "\nTesting OAuth patterns...\n"
test_hook_blocks "OAuth client secret" 'client_secret = "1234567890abcdef1234567890abcdef"'

# Test generic high-entropy secrets
printf "\nTesting generic patterns...\n"
test_hook_blocks "Generic secret" 'secret = "a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8s9t0"'

# Test UUID/GUID patterns
printf "\nTesting UUID patterns...\n"
test_hook_blocks "UUID in URL" "https://example.com/auth/12345678-1234-1234-1234-123456789abc"
test_hook_blocks "GUID assignment" "tenant_id = abcdef12-3456-7890-abcd-ef1234567890"
test_hook_blocks "UUID constant" "const CLIENT_ID = '11111111-2222-3333-4444-555555555555'"

# Test PII patterns
printf "\nTesting PII patterns...\n"
test_hook_blocks "Email address" "user_email = john.doe@example.com"
test_hook_blocks "Phone number (formatted)" "phone = (555) 123-4567"
test_hook_blocks "Phone number (dashes)" "contact = 555-123-4567"
test_hook_blocks "Phone number (plain)" "phone_number = 5551234567"
test_hook_blocks "SSN (formatted)" "ssn = 123-45-6789"
test_hook_blocks "SSN (plain)" "social_security_number = 123456789"
test_hook_blocks "Credit card (spaces)" "cc = 4532 1234 5678 9010"
test_hook_blocks "Credit card (dashes)" "card_number = 4532-1234-5678-9010"
test_hook_blocks "IPv4 address" "server_ip = 192.168.1.100"
test_hook_blocks "IPv6 address" "ipv6 = 2001:0db8:85a3:0000:0000:8a2e:0370:7334"

# Test clean commits (should be allowed)
printf "\nTesting clean commits...\n"
test_hook_allows "# This is a clean comment"
test_hook_allows "const API_URL = 'https://api.example.com'"
test_hook_allows "password_field = 'password'  # Field name, not actual password"
test_hook_allows "const SECRET_LENGTH = 32  # Configuration constant"

# Summary
printf "\n==================================================\n"
printf "Test Results:\n"
printf "  Passed: %d\n" "$tests_passed"
printf "  Failed: %d\n" "$tests_failed"

if [ "$tests_failed" -gt 0 ]; then
  printf "\n✗ Some tests failed!\n"
  exit 1
else
  printf "\n✓ All tests passed!\n"
  exit 0
fi
