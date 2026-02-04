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

  # Try to commit (should fail)
  if git commit -m "Test commit with $pattern_type" 2>&1 | grep -q "Potential sensitive information detected"; then
    pass "Hook blocked commit with $pattern_type"
    git reset HEAD test-file.txt > /dev/null 2>&1 || true
    rm -f test-file.txt
    return 0
  else
    fail "Hook failed to block commit with $pattern_type"
    git reset HEAD test-file.txt > /dev/null 2>&1 || true
    rm -f test-file.txt
    return 1
  fi
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

printf "Testing pre-commit hook sensitive pattern detection\n"
printf "==================================================\n\n"

# Test AWS credentials
printf "Testing AWS patterns...\n"
test_hook_blocks "AWS access key" "aws_access_key_id = AKIAIOSFODNN7EXAMPLE"
test_hook_blocks "AWS key ID format" "export AWS_KEY=AKIAIOSFODNN7EXAMPLE"

# Test GitHub tokens
printf "\nTesting GitHub patterns...\n"
test_hook_blocks "GitHub PAT" "token = ghp_1234567890123456789012345678901234567890"
test_hook_blocks "GitHub token assignment" "GITHUB_TOKEN=gho_abcdefghijklmnopqrstuvwxyz12345678901234"

# Test API keys
printf "\nTesting API key patterns...\n"
test_hook_blocks "API key" 'apikey = "1234567890abcdef1234567890abcdef"'
test_hook_blocks "Secret key" 'secret_key: "abcdef1234567890abcdef1234567890"'

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
