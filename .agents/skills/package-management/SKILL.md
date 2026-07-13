---
name: package-management
description: >
  System package resource and provider conventions for pacman, AUR helpers, and
  winget. Use when changing package config, state discovery, or installation.
---

# Package Management

System packages are declared in `conf/packages.toml`, parsed by
`cli/src/config/packages.rs`, represented by package resources, and converged by
tasks under `cli/src/tasks/packages/`.

## Configuration Contract

Package entries support a concise name or structured metadata:

```toml
[arch]
packages = [
  "git",
  { name = "powershell-bin", aur = true },
]
```

Use exact manager identifiers. Keep AUR intent in structured metadata rather
than inferring it from package names.

## Architecture

| Responsibility | Owner |
|---|---|
| deserialize and validate entries | `cli/src/config/packages.rs` |
| manager-specific query/install behavior | `PackageProvider` implementations |
| resource state and mutation | package resource module |
| applicability, policy, and orchestration | package tasks |

`PackageManager` selects a provider. Add a provider implementation rather than
branching manager-specific command logic through tasks.

## Convergence Flow

1. Select applicable entries and manager from platform capabilities.
2. Verify the manager executable is available.
3. Query installed packages once per manager.
4. Share the cached state with package resources.
5. Let `PackageInstallOperation` and `process_operation()` plan and converge the
   missing entries.
6. Preserve provider-level batching where supported; otherwise install
   individually.

Do not run one installed-state command per package. Keep manager commands behind
the executor abstraction and preserve idempotent manager options.

## Platform Rules

- Pacman handles ordinary Arch packages.
- The AUR helper bootstrap and AUR package task remain separate from ordinary
  package installation.
- AUR commands must not add an extra sudo layer around tools that manage their
  own elevation.
- Winget uses exact package IDs and may require per-package installation.
- Prefer platform capability methods over direct OS checks.

When a package manager is unavailable, return an explicit skipped result or
capability diagnostic consistent with task policy; do not silently report
success.

## Change Checklist

1. Update config parsing and validation for new metadata.
2. Extend `PackageManager` and add a focused provider when adding a manager.
3. Preserve one-query state discovery and batch behavior.
4. Route subprocesses through `ctx.executor`.
5. Add provider command, state mapping, missing-manager, and dry-run tests.
6. Review Linux and Windows behavior.

## Validation

- Use `resource-implementation` for resource/provider changes.
- Use `windows-specific-patterns` for winget or Windows behavior.
- Use `cross-platform-verification` after Rust changes.

## Rules

- Package installation must be idempotent.
- Installed state is queried once per manager.
- Manager-specific behavior stays in providers.
- Missing capabilities are surfaced, not swallowed.
- Configuration remains the source of desired package state.
