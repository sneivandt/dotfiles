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

System packages are declared in `conf/packages.toml`, loaded by `cli/src/config/packages.rs`, and installed by tasks in `cli/src/tasks/packages.rs`.

## Configuration

```toml
[arch]
packages = [
  "git",
  "neovim",
  { name = "powershell-bin", aur = true },
]

[arch-desktop]
packages = [
  "alacritty",
  { name = "visual-studio-code-insiders-bin", aur = true },
]

[windows]
packages = [
  "Git.Git",
  "Microsoft.PowerShell",
]
```

Packages with `aur = true` are tagged with `is_aur = true` in the loaded config.

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

### PackageProvider Trait

All package manager operations are abstracted behind the `PackageProvider` trait
(`resources/package.rs`). Each implementation encapsulates one package manager's
CLI, and new managers require only a new `PackageProvider` impl and a
`PackageManager` enum variant:

```rust
pub trait PackageProvider: std::fmt::Debug + Send + Sync {
    fn name(&self) -> &'static str;
    fn query_installed(&self, executor: &dyn Executor) -> Result<HashSet<String>>;
    fn is_installed(&self, name: &str, executor: &dyn Executor) -> Result<bool>;
    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange>;
    fn supports_batch(&self) -> bool { false }
    fn batch_install(&self, names: &[&str], executor: &dyn Executor) -> Result<()> { ... }
}
```

Concrete implementations:
- `PacmanProvider` — `pacman -S --needed --noconfirm`
- `ParuProvider` — `paru -S --needed --noconfirm` (delegates `query_installed` to `PacmanProvider`)
- `WingetProvider` — `winget install --id --exact` (no batch support)

### PackageManager::provider()

The `PackageManager` enum maps to its provider via `provider()`:

```rust
let provider: &'static dyn PackageProvider = PackageManager::Pacman.provider();
```

Providers are `&'static` (zero-cost, no `Arc`). `PackageResource` stores both
the `PackageManager` enum and its `provider` reference.

### Resource-Based Package Installation

All package managers use `process_resource_states()` with batch-queried state.
The `batch_install_packages()` function groups resources by manager and delegates
to each provider:

```rust
let installed = get_installed_packages(manager, &*ctx.executor)?;
// Build (PackageResource, ResourceState) pairs using state_from_installed()
// Then call process_resource_states() for dry-run/apply logic
// Finally batch_install_packages() for the actual install
```

Providers that support batch installation (pacman, paru) install all missing
packages in one command; providers that do not (winget) install individually.

### Batch State Checking

For efficiency, all installed packages are queried once via `get_installed_packages(manager, executor)`,
which delegates to `provider.query_installed()`, then each resource checks
membership via `state_from_installed(&installed)` (a `HashSet` lookup).
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
3. Mark AUR packages with `aur = true` in structured metadata format: `{ name = "pkg", aur = true }`
4. Use exact package IDs on Windows (case-sensitive)
5. Return `TaskResult::Skipped` with reason when package manager is missing
