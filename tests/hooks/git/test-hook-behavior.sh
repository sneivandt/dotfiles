#!/bin/sh
# shellcheck shell=sh

test_hook_wiring() {
  repo_root="$REPO_ROOT"

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
printf '%s full=%s\n' '$script' "\${DOTFILES_HOOKS_FULL:-0}" >> "\$HOOK_LOG"
EOF
  done

  (
    cd "$repo" || exit 1
    HOOK_LOG="$hook_log" sh hooks/pre-commit
  )

  expected=$(printf '%s\n' \
    'check-sensitive.sh full=0' \
    'check-rust.sh full=0' \
    'check-ci-guards.sh full=0')
  if [ "$(cat "$hook_log")" = "$expected" ]; then
    pass "Default hook runs sensitive-data, Rust, and fast CI guard checks"
  else
    fail "Default hook did not run the expected checks"
  fi

  : > "$hook_log"
  (
    cd "$repo" || exit 1
    HOOK_LOG="$hook_log" DOTFILES_HOOKS_FULL=1 sh hooks/pre-commit
  )

  expected=$(printf '%s\n' \
    'check-sensitive.sh full=1' \
    'check-rust.sh full=1' \
    'check-ci-guards.sh full=1')
  if [ "$(cat "$hook_log")" = "$expected" ]; then
    pass "Full hook mode runs all helper scripts with full mode enabled"
  else
    fail "Full hook mode did not run all helper scripts"
  fi

  rm -rf "$repo"
}

test_release_workflow_guards() {
  repo_root="${DIR:-$(git rev-parse --show-toplevel)}"
  repo="$(mktemp -d)"

  git init -q "$repo"
  mkdir -p "$repo/hooks" "$repo/.github/workflows/scripts/linux"
  cp "$repo_root/hooks/check-ci-guards.sh" "$repo/hooks/check-ci-guards.sh"
  cp "$repo_root/.github/workflows/scripts/linux/check-ci-contract.py" \
    "$repo/.github/workflows/scripts/linux/check-ci-contract.py"
  cp "$repo_root/.github/workflows/release.yml" "$repo/.github/workflows/release.yml"
  git -C "$repo" add .github/workflows/release.yml

  if (
    cd "$repo" || exit 1
    sh hooks/check-ci-guards.sh >/dev/null 2>&1
  ); then
    pass "Release workflow guards accept the hardened workflow"
  else
    fail "Release workflow guards rejected the hardened workflow"
  fi

  sed '/^[[:space:]]*workflow_run:/i\
  workflow_dispatch:' \
    "$repo_root/.github/workflows/release.yml" > "$repo/.github/workflows/release.yml"
  git -C "$repo" add .github/workflows/release.yml

  if (
    cd "$repo" || exit 1
    sh hooks/check-ci-guards.sh >/dev/null 2>&1
  ); then
    fail "Release workflow guards accepted a manually dispatched privileged workflow"
  else
    pass "Release workflow guards reject manually dispatched privileged workflows"
  fi

  sed '/^[[:space:]]*- name: Verify attestation discoverability[[:space:]]*$/,/^[[:space:]]*- name: Create release[[:space:]]*$/{ /^[[:space:]]*- name: Create release[[:space:]]*$/!d; }' \
    "$repo_root/.github/workflows/release.yml" > "$repo/.github/workflows/release.yml"
  git -C "$repo" add .github/workflows/release.yml

  if (
    cd "$repo" || exit 1
    sh hooks/check-ci-guards.sh >/dev/null 2>&1
  ); then
    fail "Release workflow guards accepted publication without proving attestation discoverability"
  else
    pass "Release workflow guards require attestation verification before publication"
  fi

  sed '/^[[:space:]]*target_commitish:[[:space:]]*/d' \
    "$repo_root/.github/workflows/release.yml" > "$repo/.github/workflows/release.yml"
  git -C "$repo" add .github/workflows/release.yml

  if (
    cd "$repo" || exit 1
    sh hooks/check-ci-guards.sh >/dev/null 2>&1
  ); then
    fail "Release workflow guards accepted a tag without the tested commit target"
  else
    pass "Release workflow guards require tags to target the tested commit"
  fi

  sed '/^[[:space:]]*retention-days:[[:space:]]*1[[:space:]]*$/d' \
    "$repo_root/.github/workflows/release.yml" > "$repo/.github/workflows/release.yml"
  git -C "$repo" add .github/workflows/release.yml

  if (
    cd "$repo" || exit 1
    sh hooks/check-ci-guards.sh >/dev/null 2>&1
  ); then
    fail "Release workflow guards accepted long-lived handoff artifacts"
  else
    pass "Release workflow guards require one-day handoff artifact retention"
  fi

  sed '/^[[:space:]]*group:[[:space:]]*release[[:space:]]*$/d' \
    "$repo_root/.github/workflows/release.yml" > "$repo/.github/workflows/release.yml"
  git -C "$repo" add .github/workflows/release.yml
  cp "$repo_root/.github/workflows/release.yml" "$repo/.github/workflows/release.yml"

  if (
    cd "$repo" || exit 1
    sh hooks/check-ci-guards.sh >/dev/null 2>&1
  ); then
    fail "Release workflow guards read the safe working tree instead of the unsafe index"
  else
    pass "Release workflow guards validate the staged workflow"
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
  git -C "$repo" remote add upstream "$repo/offline.git"
  git -C "$repo" symbolic-ref refs/remotes/upstream/HEAD refs/remotes/upstream/main

  if (
    cd "$repo" || exit 1
    printf "refs/heads/feature %s refs/heads/main %s\n" "$sha" "$zero" |
      sh "$hook" upstream "$repo/offline.git" >/dev/null 2>&1
  ); then
    fail "Pre-push hook allowed a protected branch on the actual upstream remote"
  else
    pass "Pre-push hook blocks protected branches on the actual remote"
  fi

  if (
    cd "$repo" || exit 1
    printf "refs/heads/main %s refs/heads/main %s\n" "$sha" "$zero" |
      sh "$hook" origin https://github.com/ci-owner/repo.git >/dev/null 2>&1
  ); then
    pass "Pre-push hook allows protected branches on owned repositories"
  else
    fail "Pre-push hook blocked an owned repository"
  fi

  if (
    cd "$repo" || exit 1
    printf "refs/heads/feature %s refs/heads/feature %s\n" "$sha" "$zero" |
      sh "$hook" upstream "$repo/offline.git" >/dev/null 2>&1
  ); then
    pass "Pre-push hook allows non-protected branches"
  else
    fail "Pre-push hook blocked a non-protected branch"
  fi

  rm -rf "$repo"
}

