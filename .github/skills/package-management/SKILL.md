---
name: package-management
description: >
  Package installation patterns for the dotfiles project.
  Use when working with system packages on Linux (pacman/AUR) or Windows (winget).
metadata:
  author: sneivandt
  version: "2.0"
---

# Package Management

System packages are declared in `conf/packages.ini`, loaded by `cli/src/config/packages.rs`, and installed by tasks in `cli/src/tasks/packages.rs`.

## Configuration

```ini
[arch]
git
neovim
aur:powershell-bin

[arch,desktop]
alacritty
aur:visual-studio-code-insiders-bin

[windows]
Git.Git
Microsoft.PowerShell
```

The config loader strips the `aur:` prefix and tags those packages with `is_aur = true`.

## Task Structure

Three tasks handle package installation:

### `InstallPackages` — pacman / winget
```rust
impl Task for InstallPackages {
    fn name(&self) -> &str { "Install packages" }
    fn should_run(&self, ctx: &Context) -> bool {
        ctx.config_read().packages.iter().any(|p| !p.is_aur)
    }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        let all_packages = ctx.config_read().packages.clone();
        let packages: Vec<&Package> = all_packages.iter().filter(|p| !p.is_aur).collect();
        let manager = if ctx.platform.is_linux() { PackageManager::Pacman }
                      else { PackageManager::Winget };
        process_packages(ctx, &packages, manager)
    }
}
```

### `InstallParu` — AUR helper bootstrap
Runs only on Arch when `paru` is missing. Clones `paru-bin` from AUR and builds with `makepkg`.

### `InstallAurPackages` — paru
Runs only on Arch with `paru` installed. Uses `paru -S --needed --noconfirm`.

## Implementation Patterns

### Resource-Based Package Installation

All package managers use a shared `process_packages()` helper that batch-queries
installed packages once and then processes each package via `process_resource_states()`:

```rust
let installed = get_installed_packages(manager, &*ctx.executor)?;
let resource_states = packages.iter().map(|pkg| {
    let resource = PackageResource::new(pkg.name.clone(), manager, &*ctx.executor);
    let state = resource.state_from_installed(&installed);
    (resource, state)
});
process_resource_states(ctx, resource_states, &ProcessOpts::apply_all("install").no_bail())
```

### Batch State Checking

For efficiency, all installed packages are queried once via `get_installed_packages(manager, executor)`,
then each resource checks membership via `state_from_installed(&installed)` (a `HashSet` lookup).
This avoids running one command per package.

## Key Patterns

- **Batch state check**: Query all installed packages once, then check each via `HashSet` lookup
- **Executor injection**: `PackageResource` and `get_installed_packages()` take `&dyn Executor`
- **Idempotent**: `--needed` flag skips already-installed packages
- **Guard with `executor.which()`**: Skip gracefully if package manager not found
- **No sudo for paru**: paru manages elevation internally
- **`run_unchecked` for winget**: Allows handling non-zero exits without failing the task

## Rules

1. Always check `ctx.executor.which("pacman")` / `ctx.executor.which("winget")` before calling
2. Use `--needed --noconfirm` for pacman to ensure idempotency
3. AUR packages must be prefixed with `aur:` to be tagged correctly
4. Use exact package IDs on Windows (case-sensitive)
5. Return `TaskResult::Skipped` with reason when package manager is missing
