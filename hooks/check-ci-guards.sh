#!/bin/sh
#
# Runs fast targeted local checks for files that have caused recent CI failures.
# This complements check-rust.sh, which owns Rust fmt/clippy/test checks.
#
# Full-mode checks run only when DOTFILES_HOOKS_FULL=1 so ordinary commits stay
# fast.
#
# Can be run standalone or called from the pre-commit hook.
# Usage: sh check-ci-guards.sh

set -o errexit
set -o nounset

RED=$(printf '\033[0;31m')
YELLOW=$(printf '\033[1;33m')
DIM=$(printf '\033[2m')
NC=$(printf '\033[0m')

if git rev-parse --verify HEAD >/dev/null 2>&1; then
  against=HEAD
else
  against=$(git hash-object -t tree /dev/null)
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
# Renames are included (lowercase 'd' excludes only deletions) so that renaming
# a conf/ or symlinks/ file still triggers configuration validation.
STAGED=$(git diff --cached --name-only --diff-filter=d "$against")
MANIFEST="$REPO_ROOT/cli/Cargo.toml"

full_checks_enabled() {
  case "${DOTFILES_HOOKS_FULL:-0}" in
    1 | true | yes) return 0 ;;
    *) return 1 ;;
  esac
}

has_staged_match() {
  pattern="$1"
  printf '%s\n' "$STAGED" | grep -Eq "$pattern"
}

staged_shell_files() {
  printf '%s\n' "$STAGED" \
    | grep -E '(^dotfiles\.sh$|^install\.sh$|\.sh$|^hooks/pre-commit$|^hooks/[^/]+\.sh$)' \
    | while IFS= read -r file; do
        [ -f "$REPO_ROOT/$file" ] && printf '%s\n' "$REPO_ROOT/$file"
      done
}

abort_with_hint() {
  message="$1"
  hint="$2"
  printf '\n%s======================================================%s\n' "$RED" "$NC"
  printf '%sCommit aborted: %s%s\n' "$RED" "$message" "$NC"
  printf '%sRun:%s\n' "$YELLOW" "$NC"
  printf "  %s\n" "$hint"
  printf '%sor bypass with:%s\n' "$YELLOW" "$NC"
  printf "  git commit --no-verify\n\n"
  exit 1
}

run_config_validation() {
  if ! command -v cargo >/dev/null 2>&1; then
    printf '%sSkipping typed config validation: cargo not installed.%s\n' "$DIM" "$NC"
    return
  fi

  printf "Running typed config validation...\n"
  if ! cargo run --quiet --profile ci --manifest-path "$MANIFEST" -- \
    check --root "$REPO_ROOT" -p desktop \
    --only config-warnings,symlink-sources,config-files,manifest-sync 2>&1; then
    abort_with_hint \
      "configuration validation failed." \
      "cargo run --profile ci --manifest-path cli/Cargo.toml -- check --root . -p desktop --only config-warnings,symlink-sources,config-files,manifest-sync"
  fi

  if full_checks_enabled; then
    printf "Running config drift integration test...\n"
    if ! cargo test --profile ci --manifest-path "$MANIFEST" --test config_drift 2>&1; then
      abort_with_hint \
        "config drift tests failed." \
        "cargo test --profile ci --manifest-path cli/Cargo.toml --test config_drift"
    fi
  else
    printf '%sSkipping config drift integration test: set DOTFILES_HOOKS_FULL=1 to run it.%s\n' "$DIM" "$NC"
  fi
}

run_dependency_guards() {
  if grep -nE '=[[:space:]]*"\*"' "$MANIFEST"; then
    abort_with_hint \
      "Cargo.toml contains a wildcard dependency." \
      "replace wildcard dependency versions in cli/Cargo.toml"
  fi

  if full_checks_enabled && cargo deny --version >/dev/null 2>&1; then
    printf "Running cargo-deny bans/licenses/sources checks...\n"
    if ! cargo deny --manifest-path "$MANIFEST" check bans licenses sources 2>&1; then
      abort_with_hint \
        "cargo-deny reported dependency policy violations." \
        "cargo deny --manifest-path cli/Cargo.toml check bans licenses sources"
    fi
  elif full_checks_enabled; then
    printf '%sSkipping cargo-deny: cargo-deny not installed.%s\n' "$DIM" "$NC"
    printf '%s  Install: cargo install cargo-deny --locked%s\n' "$DIM" "$NC"
  else
    printf '%sSkipping cargo-deny: set DOTFILES_HOOKS_FULL=1 to run it.%s\n' "$DIM" "$NC"
  fi
}

run_shell_guards() {
  if command -v shellcheck >/dev/null 2>&1; then
    printf "Running ShellCheck...\n"
    shell_files="$(staged_shell_files)"
    if [ -z "$shell_files" ]; then
      return 0
    fi
    # shellcheck disable=SC2086  # intentional word splitting of newline-free paths
    if ! shellcheck --severity=warning --shell=sh $shell_files; then
      abort_with_hint \
        "ShellCheck reported issues." \
        "shellcheck --severity=warning --shell=sh <staged shell files>"
    fi
  else
    printf '%sSkipping ShellCheck: shellcheck not installed.%s\n' "$DIM" "$NC"
  fi
}

run_wrapper_guards() {
  printf "Running Linux shell wrapper tests...\n"
  export DIR="$REPO_ROOT"
  if [ -z "${BINARY_PATH:-}" ]; then
    if [ -x "$REPO_ROOT/cli/target/dev-opt/dotfiles" ]; then
      BINARY_PATH="$REPO_ROOT/cli/target/dev-opt/dotfiles"
    elif [ -x "$REPO_ROOT/cli/target/ci/dotfiles" ]; then
      BINARY_PATH="$REPO_ROOT/cli/target/ci/dotfiles"
    else
      BINARY_PATH=""
    fi
  fi
  export BINARY_PATH
  scripts_dir="$REPO_ROOT/.github/workflows/scripts/linux"
  if ! sh "$scripts_dir/test-shell-wrapper.sh"; then
    abort_with_hint \
      "Linux shell wrapper tests failed." \
      "DIR=\"\$(git rev-parse --show-toplevel)\" BINARY_PATH=\"\" sh .github/workflows/scripts/linux/test-shell-wrapper.sh"
  fi
}

require_workflow_pattern() {
  pattern="$1"
  description="$2"
  if ! printf '%s\n' "$workflow_contents" | grep -Eq "$pattern"; then
    abort_with_hint \
      "release workflow is missing $description." \
      "restore the publishing invariant in .github/workflows/release.yml"
  fi
}

run_release_workflow_guards() {
  printf "Running release workflow guards...\n"

  if ! workflow_contents=$(git show ":.github/workflows/release.yml"); then
    abort_with_hint \
      "staged release workflow cannot be read." \
      "stage .github/workflows/release.yml before running the publishing guards"
  fi

  require_workflow_pattern '^[[:space:]]+group:[[:space:]]+release[[:space:]]*$' "serialized release concurrency"
  require_workflow_pattern '^[[:space:]]+cancel-in-progress:[[:space:]]+false[[:space:]]*$' "non-cancelling release concurrency"
  require_workflow_pattern '^[[:space:]]+actions:[[:space:]]+read[[:space:]]*$' "read access to CI run metadata"
  require_workflow_pattern '^[[:space:]]+ci_run_id:[[:space:]]*$' "the manual CI run input"
  require_workflow_pattern '\.github/workflows/ci\.yml' "canonical CI workflow validation"
  require_workflow_pattern 'needs\.check-ci\.outputs\.sha' "the validated CI commit handoff"
  require_workflow_pattern '^[[:space:]]+id-token:[[:space:]]+write[[:space:]]*$' "OIDC permission for provenance signing"
  require_workflow_pattern '^[[:space:]]+attestations:[[:space:]]+write[[:space:]]*$' "attestation upload permission"
  require_workflow_pattern 'actions/attest-build-provenance@' "the build provenance action"
  require_workflow_pattern '^[[:space:]]+dotfiles-linux-x86_64[[:space:]]*$' "the Linux x86_64 attestation subject"
  require_workflow_pattern '^[[:space:]]+dotfiles-linux-aarch64[[:space:]]*$' "the Linux aarch64 attestation subject"
  require_workflow_pattern '^[[:space:]]+dotfiles-windows-x86_64\.exe[[:space:]]*$' "the Windows attestation subject"
  require_workflow_pattern 'gh attestation verify "\$artifact" --repo "\$GITHUB_REPOSITORY"' "the attestation discoverability check"
  require_workflow_pattern 'target_commitish:[[:space:]]+\$\{\{ needs\.version\.outputs\.sha \}\}' "the exact tested release tag target"

  attest_line=$(printf '%s\n' "$workflow_contents" | grep -nF -- '- name: Attest build provenance' | head -n 1 | cut -d: -f1)
  verify_line=$(printf '%s\n' "$workflow_contents" | grep -nF -- '- name: Verify attestation discoverability' | head -n 1 | cut -d: -f1)
  release_line=$(printf '%s\n' "$workflow_contents" | grep -nF -- '- name: Create release' | head -n 1 | cut -d: -f1)
  if [ -z "$attest_line" ] || [ -z "$verify_line" ] || [ -z "$release_line" ] ||
    [ "$attest_line" -ge "$verify_line" ] || [ "$verify_line" -ge "$release_line" ]; then
    abort_with_hint \
      "release provenance is not verified before publication." \
      "keep the attest, discoverability verification, and release steps in that order"
  fi

  if printf '%s\n' "$workflow_contents" | grep -Fq 'github.event.workflow_run.head_sha || github.sha'; then
    abort_with_hint \
      "manual releases can fall back to an unverified workflow commit." \
      "use needs.check-ci.outputs.sha for every release checkout and build"
  fi
}

require_docker_workflow_pattern() {
  pattern="$1"
  description="$2"
  if ! printf '%s\n' "$docker_workflow_contents" | grep -Eq "$pattern"; then
    abort_with_hint \
      "Docker workflow is missing $description." \
      "restore the publishing invariant in .github/workflows/docker.yml"
  fi
}

require_docker_workflow_literal() {
  literal="$1"
  description="$2"
  if ! printf '%s\n' "$docker_workflow_contents" | grep -Fq "$literal"; then
    abort_with_hint \
      "Docker workflow is missing $description." \
      "restore the publishing invariant in .github/workflows/docker.yml"
  fi
}

require_dockerfile_pattern() {
  pattern="$1"
  description="$2"
  if ! printf '%s\n' "$dockerfile_contents" | grep -Eq "$pattern"; then
    abort_with_hint \
      "Dockerfile is missing $description." \
      "restore the publishing invariant in Dockerfile"
  fi
}

run_docker_publish_guards() {
  printf "Running Docker publishing guards...\n"

  if ! docker_workflow_contents=$(git show ":.github/workflows/docker.yml"); then
    abort_with_hint \
      "staged Docker workflow cannot be read." \
      "stage .github/workflows/docker.yml before running the publishing guards"
  fi
  if ! dockerfile_contents=$(git show ":Dockerfile"); then
    abort_with_hint \
      "staged Dockerfile cannot be read." \
      "stage Dockerfile before running the publishing guards"
  fi
  if ! rust_toolchain_contents=$(git show ":rust-toolchain.toml"); then
    abort_with_hint \
      "staged Rust toolchain cannot be read." \
      "stage rust-toolchain.toml before running the publishing guards"
  fi

  rust_toolchain_version=$(printf '%s\n' "$rust_toolchain_contents" |
    sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)"[[:space:]]*$/\1/p' |
    head -n 1)
  if ! printf '%s\n' "$rust_toolchain_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    abort_with_hint \
      "rust-toolchain.toml does not pin a complete Rust version." \
      "set toolchain.channel to a version such as 1.95.0"
  fi

  require_docker_workflow_pattern '^[[:space:]]+group:[[:space:]]+docker-main[[:space:]]*$' "serialized main publishing"
  require_docker_workflow_pattern '^[[:space:]]+cancel-in-progress:[[:space:]]+true[[:space:]]*$' "superseded-run cancellation"
  require_docker_workflow_literal 'ref: ${{ github.event.workflow_run.head_sha }}' "the tested commit checkout"
  require_docker_workflow_literal 'persist-credentials: false' "non-persistent checkout credentials"
  require_docker_workflow_literal 'DOTFILES_VERSION=sha-${{ github.event.workflow_run.head_sha }}' "the tested commit binary version"
  require_docker_workflow_literal 'sneivandt/dotfiles:sha-${{ github.event.workflow_run.head_sha }}' "the immutable commit tag"

  require_dockerfile_pattern '^FROM[[:space:]]+rust:[0-9]+\.[0-9]+\.[0-9]+-bookworm@sha256:[0-9a-f]{64}[[:space:]]+AS[[:space:]]+builder$' "the pinned Rust builder image"
  require_dockerfile_pattern '^FROM[[:space:]]+ubuntu:24\.04@sha256:[0-9a-f]{64}$' "the pinned runtime image"
  require_dockerfile_pattern '^ENV[[:space:]]+CARGO_TARGET_DIR=/build/target$' "the out-of-source Cargo target directory"
  require_dockerfile_pattern 'update-ref refs/remotes/origin/main HEAD' "the tested commit upstream reference"
  require_dockerfile_pattern 'cargo build --release --locked --manifest-path cli/Cargo\.toml' "the locked release build"
  require_dockerfile_pattern 'PATH=/home/sneivandt/\.local/bin:\$\{PATH\}' "the installed launcher PATH"
  require_dockerfile_pattern '^COPY --from=builder .* /build/out/dotfiles /home/sneivandt/dotfiles/bin/dotfiles$' "the staged release binary copy"

  builder_image=$(printf '%s\n' "$dockerfile_contents" |
    awk '$1 == "FROM" && $2 ~ /^rust:/ && $3 == "AS" && $4 == "builder" { print $2; exit }')
  docker_rust_version=${builder_image#rust:}
  docker_rust_version=${docker_rust_version%-bookworm@sha256:*}
  if [ "$docker_rust_version" != "$rust_toolchain_version" ]; then
    abort_with_hint \
      "Docker builder Rust $docker_rust_version does not match rust-toolchain.toml $rust_toolchain_version." \
      "update the Docker builder and rust-toolchain.toml together"
  fi

  if ! printf '%s\n' "$dockerfile_contents" | grep -Eq '^[[:space:]]*RUN[[:space:]]+DOTFILES_SKIP_SELF_UPDATE=1'; then
    abort_with_hint \
      "Docker image construction can replace its exact-source binary through self-update." \
      "set DOTFILES_SKIP_SELF_UPDATE=1 only on the Dockerfile install RUN instruction"
  fi
}

if has_staged_match '^(conf/.*\.toml|symlinks/)'; then
  run_config_validation
fi

if has_staged_match '^(cli/Cargo\.toml|cli/Cargo\.lock|cli/deny\.toml)$'; then
  run_dependency_guards
fi

if has_staged_match '(^dotfiles\.sh$|\.sh$|^hooks/)'; then
  run_shell_guards
fi

if full_checks_enabled && has_staged_match '(^dotfiles\.sh$|^\.github/workflows/scripts/linux/test-shell-wrapper\.sh$)'; then
  run_wrapper_guards
elif has_staged_match '(^dotfiles\.sh$|^\.github/workflows/scripts/linux/test-shell-wrapper\.sh$)'; then
  printf '%sSkipping Linux shell wrapper tests: set DOTFILES_HOOKS_FULL=1 to run them.%s\n' "$DIM" "$NC"
fi

if has_staged_match '^\.github/workflows/release\.yml$'; then
  run_release_workflow_guards
fi

if has_staged_match '^(\.github/workflows/docker\.yml|Dockerfile|rust-toolchain\.toml)$'; then
  run_docker_publish_guards
fi

exit 0
