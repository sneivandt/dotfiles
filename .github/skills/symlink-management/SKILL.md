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

Symlinks connect config files from `symlinks/` to `$HOME`. Config in `conf/symlinks.toml`, loaded by `config::symlinks`, installed by `tasks::symlinks`.

## Configuration

```ini
[base]
config/nvim
config/git/config
[arch,desktop]
config/xmonad
[windows]
{ source = "AppData/Roaming/Code/User/settings.json", target = "AppData/Roaming/Code/User/settings.json" }
"config/git/windows"
```

Source files in `symlinks/` have **no leading dots**.

## Target Path

`compute_target()` in `phases/apply/symlinks.rs` always prepends a dot:

```rust
fn compute_target(home: &Path, source: &str) -> PathBuf {
    home.join(format!(".{source}"))
}
```

For paths that must **not** receive a dot prefix (Windows paths like `AppData/` or
`Documents/`), use an explicit `target` field in `conf/symlinks.toml`:

```toml
{ source = "AppData/Roaming/Code/User/settings.json", target = "AppData/Roaming/Code/User/settings.json" }
```

The explicit target is joined to `$HOME` directly: `home.join(target)`.

## Task Implementation

The task uses `SymlinkResource` from the `resources` module for declarative state
management via the generic `process_resources()` helper:

```rust
#[derive(Debug)]
pub struct InstallSymlinks;
impl Task for InstallSymlinks {
    fn name(&self) -> &str { "Install symlinks" }
    task_deps![
        crate::phases::repository::reload_config::ReloadConfig,
        crate::phases::bootstrap::developer_mode::EnableDeveloperMode,
    ];
    fn should_run(&self, ctx: &Context) -> bool { !ctx.config_read().symlinks.is_empty() }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        process_resources(ctx, build_resources(ctx), &ProcessOpts::strict("link"))
    }
}
```

Platform-specific symlink creation is handled inside `SymlinkResource::apply()`.

## Adding Symlinks

1. Create source: `symlinks/config/myapp/config` (no leading dot)
2. Add to `conf/symlinks.toml` under correct profile section:
   - Plain string `"config/myapp/config"` → target `~/.config/myapp/config` (dot prepended automatically)
   - `{ source = "AppData/Roaming/MyApp/config", target = "AppData/Roaming/MyApp/config" }` → target `~/AppData/Roaming/MyApp/config` (no dot prefix)
3. Optionally add to `conf/manifest.toml` for sparse checkout
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
    process_resources_remove(ctx, build_resources(ctx), "unlink")
}
```

## Rules

- No leading dots in `symlinks.toml` or `symlinks/` paths
- Use directory symlinks for entire config dirs, file symlinks for selective management
- Don't create symlinks inside already-symlinked directories
- Source files filtered by sparse checkout are silently skipped
