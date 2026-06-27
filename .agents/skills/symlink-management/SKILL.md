---
name: symlink-management
description: >
  Detailed symlink conventions and management for the dotfiles project.
  Use when creating, modifying, or troubleshooting symlinks.
---

# Symlink Management

Symlinks connect config files from `symlinks/` to `$HOME`. Config in
`conf/symlinks.toml` is loaded by `config::symlinks` and installed by
`tasks::files::symlinks`.

## Configuration

```toml
[base]
symlinks = [
  "config/nvim",
  "config/git/config",
]

[arch-desktop]
symlinks = [
  "config/hypr",
]

[windows]
symlinks = [
  { source = "AppData/Roaming/Code/User/settings.json", target = "AppData/Roaming/Code/User/settings.json" },
  "config/git/windows",
]
```

Source files in `symlinks/` have **no leading dots**.
Source and explicit target paths must be relative and must not contain `..`
components; invalid paths are reported as unsafe configuration instead of being
applied.

## Target Path

`compute_target()` in `tasks/files/symlinks.rs` always prepends a dot:

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

The install task is generated with `resource_task!` and uses `SymlinkResource`
for declarative state management via `process_resources()`:

```rust
resource_task! {
    /// Create symlinks from symlinks/ to $HOME.
    pub InstallSymlinks {
        name: "Install symlinks",
        phase: TaskPhase::Provision,
        items: |ctx| ctx.config_read().symlinks.clone(),
        build: |s, ctx| {
            let repo_root = ctx.root();
            build_resource(&s, &repo_root, &ctx.home, &ctx.executor)
        },
        opts: ProcessOpts::strict("link"),
    }
}
```

Platform-specific symlink creation is handled inside `SymlinkResource::apply()`.

## Adding Symlinks

1. Create source: `symlinks/config/myapp/config` (no leading dot)
2. Add to `conf/symlinks.toml` under correct profile section:
   - Plain string `"config/myapp/config"` → target `~/.config/myapp/config` (dot prepended automatically)
   - `{ source = "AppData/Roaming/MyApp/config", target = "AppData/Roaming/MyApp/config" }` → target `~/AppData/Roaming/MyApp/config` (no dot prefix)
3. For non-`base` sections, add the source path to the matching `conf/manifest.toml` section for sparse checkout validation
4. Test: `./dotfiles.sh install -d`

## Idempotency

- Correct symlink already exists → skip
- Wrong symlink target → fix when safe
- Unsafe paths or missing sources → report as invalid and skip applying

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
- Source and explicit target paths must be relative and must not contain `..`
- Use directory symlinks for entire config dirs, file symlinks for selective management
- Don't create symlinks inside already-symlinked directories
- Missing source files are invalid and skipped without applying
