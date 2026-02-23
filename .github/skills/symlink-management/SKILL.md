---
name: symlink-management
description: >
  Detailed symlink conventions and management for the dotfiles project.
  Use when creating, modifying, or troubleshooting symlinks.
metadata:
  author: sneivandt
  version: "2.0"
---

# Symlink Management

Symlinks connect config files from `symlinks/` to `$HOME`. Config in `conf/symlinks.ini`, loaded by `config::symlinks`, installed by `tasks::symlinks`.

## Configuration

```ini
[base]
config/nvim
config/git/config
[arch,desktop]
config/xmonad
[windows]
AppData/Roaming/Code/User/settings.json
```

Source files in `symlinks/` have **no leading dots**.

## Target Path

`compute_target()` in `tasks/symlinks.rs`:
- Most paths: `$HOME/.<entry>` (dot prepended)
- Paths starting with `Documents/` (any case): `$HOME/Documents/...` (no dot)
- Paths starting with `AppData/` (any case): `$HOME/AppData/...` (no dot)

```rust
fn compute_target(home: &Path, source: &str) -> PathBuf {
    let lower = source.to_ascii_lowercase();
    if lower.starts_with("documents/") || lower.starts_with("appdata/") {
        home.join(source)
    } else {
        home.join(format!(".{source}"))
    }
}
```

## Task Implementation

The task uses `SymlinkResource` from the `resources` module for declarative state
management via the generic `process_resources()` helper:

```rust
#[derive(Debug)]
pub struct InstallSymlinks;
impl Task for InstallSymlinks {
    fn name(&self) -> &str { "Install symlinks" }
    fn dependencies(&self) -> &[TypeId] {
        const DEPS: &[TypeId] = &[
            TypeId::of::<super::reload_config::ReloadConfig>(),
            TypeId::of::<super::developer_mode::EnableDeveloperMode>(),
        ];
        DEPS
    }
    fn should_run(&self, ctx: &Context) -> bool { !ctx.config_read().symlinks.is_empty() }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_resources(ctx, build_resources(ctx), &ProcessOpts::apply_all("link"))
    }
}
```

Platform-specific symlink creation is handled inside `SymlinkResource::apply()`.

## Adding Symlinks

1. Create source: `symlinks/config/myapp/config` (no leading dot)
2. Add to `conf/symlinks.ini` under correct profile section
3. Optionally add to `conf/manifest.ini` for sparse checkout
4. Test: `./dotfiles.sh install -d`

## Idempotency

- Correct symlink already exists → skip
- Wrong symlink or file exists → remove and recreate
- Source missing (sparse checkout) → skip silently

## Uninstall

`UninstallSymlinks` uses `process_resources_remove()` to remove symlinks pointing
to repo sources:
```rust
fn run(&self, ctx: &Context) -> Result<TaskResult> {
    process_resources_remove(ctx, build_resources(ctx), "materialize")
}
```

## Rules

- No leading dots in `symlinks.ini` or `symlinks/` paths
- Use directory symlinks for entire config dirs, file symlinks for selective management
- Don't create symlinks inside already-symlinked directories
- Source files filtered by sparse checkout are silently skipped
