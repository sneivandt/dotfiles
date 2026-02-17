---
name: git-hooks-patterns
description: >
  Git hooks patterns and sensitive data detection for the dotfiles project.
  Use when working with git hooks, pre-commit checks, or security scanning.
metadata:
  author: sneivandt
  version: "2.0"
---

# Git Hooks Patterns

The project uses git hooks for pre-commit security scanning. Hook installation is handled by `cli/src/tasks/hooks.rs` which implements the `Task` trait.

## Overview

- **Pre-commit scanning**: Detect sensitive information before commits
- **Automatic installation**: `tasks::hooks::GitHooks` creates symlinks during install
- **Pattern-based detection**: Configurable patterns in `hooks/sensitive-patterns.ini`
- **Bypassable**: `git commit --no-verify` for false positives

Hooks live in `hooks/` and are symlinked to `.git/hooks/` by the Rust engine.

## Hook Installation Task

The `GitHooks` task in `cli/src/tasks/hooks.rs`:

```rust
pub struct GitHooks;

impl Task for GitHooks {
    fn name(&self) -> &str { "Git hooks" }
    fn should_run(&self, ctx: &Context) -> bool {
        ctx.hooks_dir().exists()
    }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        // Symlinks hooks/ files into .git/hooks/
    }
}
```

## Sensitive Data Detection

### Pattern Configuration

Patterns in `hooks/sensitive-patterns.ini` use INI sections and ERE regex:

```ini
[api-keys]
(apikey|api_key)[\s]*[=:]

[passwords]
(password|passwd|pwd)[\s]*[=:]

[private-keys]
-----BEGIN[\s]+(RSA|DSA|EC|OPENSSH)[\s]+PRIVATE[\s]+KEY-----
```

Categories: `api-keys`, `passwords`, `tokens`, `aws`, `private-keys`, `github`, `database`, `generic`.

### Pre-commit Hook

The `hooks/pre-commit` script is POSIX shell (`#!/bin/sh`):
1. Reads patterns from `hooks/sensitive-patterns.ini`
2. Scans `git diff --cached` for matches
3. Prints error and aborts if sensitive data found

### Bypassing

```bash
git commit --no-verify  # Use for false positives only
```

## Adding New Patterns

1. Write ERE pattern and add to appropriate section in `hooks/sensitive-patterns.ini`
2. Test: `echo "apikey=test" | grep -iE "pattern"`
3. Be specific to reduce false positives — match context around the secret

## Rules

- The pre-commit hook uses POSIX shell (`#!/bin/sh`)
- Hooks are installed as symlinks (changes apply immediately)
- Never commit real credentials
- Test patterns before committing
- The hook installation task uses `ctx.hooks_dir()` for source path
