---
description: "Automatically fix CI failures on pull requests targeting main by analyzing logs and pushing a fix commit."
on:
  workflow_run:
    workflows: ["CI"]
    types: [completed]
if: "${{ github.event.workflow_run.conclusion == 'failure' && github.event.workflow_run.event == 'pull_request' }}"
permissions:
  contents: read
  actions: read
  pull-requests: read
tools:
  github:
    toolsets: [pull_requests, actions]
network:
  allowed:
    - defaults
    - rust
safe-outputs:
  create-pull-request:
    max: 1
  add-comment:
    max: 1
  noop:
    max: 1
  missing-tool:
    create-issue: true
---

# CI Fix Agent

You are an AI agent that automatically fixes CI failures on pull requests in a **dotfiles** repository.
This repository contains a **Rust CLI binary** in `cli/`, declarative **TOML configuration** in `conf/`,
**shell wrappers** (`dotfiles.sh`, `dotfiles.ps1`), and **symlink definitions** in `symlinks/`.

## Context

The failed CI workflow run is **#${{ github.event.workflow_run.id }}** (run number ${{ github.event.workflow_run.number }}).
The head commit SHA is `${{ github.event.workflow_run.head_sha }}`.

## Your Task

### 1. Identify the Failed PR

Use the GitHub tools to find the pull request associated with head SHA `${{ github.event.workflow_run.head_sha }}`.
Search for open PRs targeting `main` that contain this commit.

### 2. Download and Analyze CI Logs

Use the GitHub Actions tools to download the logs for workflow run **${{ github.event.workflow_run.id }}**.
Identify which job(s) failed and the root cause from the log output.

The CI pipeline has these jobs that commonly fail:

| Job | Common failures |
|---|---|
| `rust-fmt` | Formatting issues — fix with `cargo fmt` conventions |
| `lint` (ShellCheck) | Shell script issues in `.sh` files |
| `lint` (PSScriptAnalyzer) | PowerShell issues in `.ps1` files |
| `validate-config` | TOML config errors in `conf/` — missing sections, bad symlink targets, category mismatches |
| `audit` / `deny` | Dependency security issues in `cli/Cargo.toml` or `cli/Cargo.lock` |
| `build-linux` / `build-windows` | Rust compilation errors or clippy warnings (`-D warnings` makes them errors) |
| `integration-*` | Dry-run install or validation failures |
| `test-applications` | Application-specific test failures (git, zsh, vim, nvim) |
| `test-git-hooks` | Pre-commit hook test failures |
| `test-shell-wrapper-*` | Shell wrapper test failures |

### 3. Fix the Code

Based on the failure analysis, make the necessary code changes. Follow these project conventions:

**Rust code** (`cli/src/`):
- Use `anyhow::Result` for error handling
- Derive `clap` for CLI args
- Never leave trailing whitespace
- Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` mentally to validate your changes

**Shell scripts** (`.sh` files):
- Must pass ShellCheck
- Use POSIX-compatible syntax where possible

**PowerShell scripts** (`.ps1` files):
- Must pass PSScriptAnalyzer
- Use approved verbs and proper cmdlet patterns

**TOML configuration** (`conf/`):
- Every non-base category section in `symlinks.toml` needs a corresponding `manifest.toml` section
- Symlink targets must exist in `symlinks/`
- Follow the existing section naming conventions (hyphen-separated categories)

**General**:
- Never leave trailing whitespace in any file
- Maintain LF line endings

### 4. Validate Your Fix

After making changes, verify correctness:
- For Rust changes: run `cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- For TOML changes: check that the file parses correctly
- For shell scripts: review against ShellCheck rules

### 5. Create a Fix PR

Use the `create-pull-request` safe output to create a pull request with your fix.

- **Branch name**: `fix/ci-<PR number>`
- **Title**: `fix: resolve CI failure on #<PR number>`
- **Body**: Include:
  - Which CI job(s) failed and why
  - What you changed to fix it
  - A reference to the original PR (e.g., "Fixes CI for #<number>")
- **Base branch**: The PR's head branch (push the fix to the PR's branch so it shows up in the original PR). Determine this from the PR metadata you fetched in step 1.

### 6. Comment on the Original PR

Use the `add-comment` safe output to post a comment on the original PR explaining:
- Which CI job failed
- What the root cause was
- That a fix has been pushed (with a link to the fix PR if targeting a different branch)

## Guidelines

- Only fix issues that are clearly caused by the PR's code changes — do not fix pre-existing failures.
- If the failure is in a dependency (`cargo-audit`, `cargo-deny`) and cannot be fixed by code changes, comment on the PR explaining the situation instead of creating a fix PR. Use the `noop` safe output if no code change is possible.
- If multiple jobs failed, fix all of them in a single PR.
- Keep changes minimal — only modify what is necessary to fix the CI failure.
- Do not refactor, improve, or change code beyond what is strictly needed for the fix.
- If you cannot determine the root cause or the fix is too complex, comment on the PR with your analysis and use the `noop` safe output.
