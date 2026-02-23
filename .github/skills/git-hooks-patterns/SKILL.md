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
- **Automatic installation**: `tasks::hooks::InstallGitHooks` copies hooks during install
- **Pattern-based detection**: Configurable patterns in `hooks/sensitive-patterns.ini`
- **Bypassable**: `git commit --no-verify` for false positives

Hooks live in `hooks/` and are copied to `.git/hooks/` by the Rust engine.

## Hook Installation Task

The `InstallGitHooks` task in `cli/src/tasks/hooks.rs`:

```rust
#[derive(Debug)]
pub struct InstallGitHooks;

impl Task for InstallGitHooks {
    fn name(&self) -> &str { "Install git hooks" }
    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[TypeId::of::<super::reload_config::ReloadConfig>()];
        DEPS
    }
    fn should_run(&self, ctx: &Context) -> bool {
        ctx.fs_ops.exists(&ctx.hooks_dir()) && ctx.fs_ops.exists(&ctx.root().join(".git"))
    }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let resources = discover_hooks(ctx)?;
        process_resources(ctx, resources, &ProcessOpts::apply_all("install hook"))
    }
}
```

`should_run` uses `ctx.fs_ops.exists()` (the `FileSystemOps` abstraction) rather than
calling `.exists()` directly on the path. This allows tests to inject a
`MockFileSystemOps` via `ctx.with_fs_ops(Arc::new(mock))` without touching the real
filesystem.

`discover_hooks()` reads the `hooks/` directory via `ctx.fs_ops.read_dir()` and returns
one `HookFileResource` per file that has no extension (conventional hook scripts such
as `pre-commit`, `commit-msg`).

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
- Hooks are installed as copies (re-run install to update after changes)
- Never commit real credentials
- Test patterns before committing
- The hook installation task uses `ctx.fs_ops.exists()` to check directory existence (not `.exists()` directly), enabling MockFileSystemOps injection in tests
