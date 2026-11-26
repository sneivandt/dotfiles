#!/bin/sh
# shellcheck shell=sh

init_hook_test_repo() {
  unset GIT_DIR GIT_COMMON_DIR GIT_WORK_TREE GIT_INDEX_FILE \
    GIT_OBJECT_DIRECTORY GIT_ALTERNATE_OBJECT_DIRECTORIES \
    GIT_CONFIG GIT_CONFIG_PARAMETERS
  GIT_CONFIG_NOSYSTEM=1
  GIT_CONFIG_COUNT=0
  GIT_CONFIG_GLOBAL="$HOOK_TEST_REPO/.empty-gitconfig"
  : > "$GIT_CONFIG_GLOBAL"
  export GIT_CONFIG_NOSYSTEM GIT_CONFIG_COUNT GIT_CONFIG_GLOBAL

  git init -q --template= "$HOOK_TEST_REPO"
  git -C "$HOOK_TEST_REPO" config user.name "Hook Test"
  git -C "$HOOK_TEST_REPO" config user.email "ci@test.local"
  mkdir -p "$HOOK_TEST_REPO/hooks" "$HOOK_TEST_REPO/.git/hooks"
  cp -a "$REPO_ROOT/hooks/." "$HOOK_TEST_REPO/hooks/"
  ln -sf "$HOOK_TEST_REPO/hooks/pre-commit" "$HOOK_TEST_REPO/.git/hooks/pre-commit"
  chmod +x "$HOOK_TEST_REPO/hooks/pre-commit"

  # Establish a parent commit so test_hook_allows can safely reset HEAD~1.
  printf 'baseline\n' > "$HOOK_TEST_REPO/baseline.txt"
  git -C "$HOOK_TEST_REPO" add baseline.txt
  git -C "$HOOK_TEST_REPO" -c core.hooksPath=/dev/null commit -qm "Baseline"
}


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

  # Create a test file with sensitive content in the disposable repository
  printf '%s\n' "$sensitive_content" > "$HOOK_TEST_REPO/test-file.txt"
  git -C "$HOOK_TEST_REPO" add test-file.txt

  # Try to commit (should fail) without echoing the detected value.
  result=1
  if commit_output=$(git -C "$HOOK_TEST_REPO" commit -m "Test commit with $pattern_type" 2>&1); then
    fail "Hook allowed commit with $pattern_type"
  elif ! printf '%s\n' "$commit_output" | grep -q "Potential sensitive information detected"; then
    fail "Hook failed to report $pattern_type"
  elif printf '%s\n' "$commit_output" | grep -Fq -- "$sensitive_content"; then
    fail "Hook printed detected $pattern_type content"
  else
    pass "Hook blocked commit with $pattern_type"
    result=0
  fi

  git -C "$HOOK_TEST_REPO" reset -q HEAD test-file.txt > /dev/null 2>&1 || true
  rm -f "$HOOK_TEST_REPO/test-file.txt"
  return "$result"
}

test_hook_blocks_staged_file_deleted_from_worktree() {
  printf 'secret_%s: "%s%s%s%s"\n' \
    'key' 'abcdef12345' '67890abcdef' '12345' '67890' > "$HOOK_TEST_REPO/staged-only-secret.txt"
  git -C "$HOOK_TEST_REPO" add staged-only-secret.txt
  rm -f "$HOOK_TEST_REPO/staged-only-secret.txt"

  if git -C "$HOOK_TEST_REPO" commit -m "Test staged-only sensitive file" 2>&1 | grep -q "Potential sensitive information detected"; then
    pass "Hook scanned a staged file deleted from the worktree"
    git -C "$HOOK_TEST_REPO" reset -q HEAD staged-only-secret.txt > /dev/null 2>&1 || true
    return 0
  fi

  fail "Hook skipped a staged file deleted from the worktree"
  git -C "$HOOK_TEST_REPO" reset -q HEAD staged-only-secret.txt > /dev/null 2>&1 || true
  return 1
}

# Test that hook allows clean commits
test_hook_allows() {
  clean_content="$1"

  # Create a test file with clean content in the disposable repository
  printf '%s\n' "$clean_content" > "$HOOK_TEST_REPO/test-file.txt"
  git -C "$HOOK_TEST_REPO" add test-file.txt

  # Try to commit (should succeed)
  if git -C "$HOOK_TEST_REPO" commit -m "Test clean commit" > /dev/null 2>&1; then
    pass "Hook allowed clean commit"
    git -C "$HOOK_TEST_REPO" reset --soft HEAD~1 > /dev/null 2>&1 || true
    git -C "$HOOK_TEST_REPO" reset -q HEAD test-file.txt > /dev/null 2>&1 || true
    rm -f "$HOOK_TEST_REPO/test-file.txt"
    return 0
  else
    fail "Hook incorrectly blocked clean commit"
    git -C "$HOOK_TEST_REPO" reset -q HEAD test-file.txt > /dev/null 2>&1 || true
    rm -f "$HOOK_TEST_REPO/test-file.txt"
    return 1
  fi
}

