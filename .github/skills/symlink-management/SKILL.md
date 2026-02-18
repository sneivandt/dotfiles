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
- `Documents/` or `documents/` paths: `$HOME/Documents/...` (no dot)
- `AppData/` paths: `$HOME/AppData/...` (no dot)

```rust
fn compute_target(home: &Path, source: &str) -> PathBuf {
    if source.starts_with("Documents/") || source.starts_with("documents/") || source.starts_with("AppData/") {
        home.join(source)
    } else {
        home.join(format!(".{source}"))
    }
}
```

## Task Implementation

The task uses the `SymlinkResource` from the `resources` module for declarative state management:

```rust
pub struct InstallSymlinks;
impl Task for InstallSymlinks {
    fn name(&self) -> &str { "Install symlinks" }
    fn should_run(&self, ctx: &Context) -> bool { !ctx.config.symlinks.is_empty() }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let mut stats = TaskStats::new();
        for symlink in &ctx.config.symlinks {
            let source = ctx.symlinks_dir().join(&symlink.source);
            let target = compute_target(&ctx.home, &symlink.source);
            let resource = SymlinkResource::new(source, target);
            match resource.current_state()? {
                ResourceState::Invalid { reason } => { stats.skipped += 1; continue; }
                ResourceState::Correct => { stats.already_ok += 1; continue; }
                ResourceState::Incorrect { .. } | ResourceState::Missing => {
                    if ctx.dry_run { stats.changed += 1; continue; }
                }
            }
            resource.apply()?;
            stats.changed += 1;
        }
        Ok(stats.finish(ctx))
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

`UninstallSymlinks` only removes symlinks pointing to repo sources:
```rust
if let Ok(link_target) = std::fs::read_link(&target) && link_target == source {
    std::fs::remove_file(&target)?;
}
```

## Rules

- No leading dots in `symlinks.ini` or `symlinks/` paths
- Use directory symlinks for entire config dirs, file symlinks for selective management
- Don't create symlinks inside already-symlinked directories
- Source files filtered by sparse checkout are silently skipped
