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

[arch,desktop]
alacritty

[arch,aur]
powershell-bin

[arch,desktop,aur]
visual-studio-code-insiders-bin

[windows]
Git.Git
Microsoft.PowerShell
```

The config loader tags packages from `[*,aur]` sections with `is_aur = true`.

## Task Structure

Three tasks handle package installation:

### `InstallPackages` — pacman / winget
```rust
impl Task for InstallPackages {
    fn name(&self) -> &str { "Install packages" }
    fn should_run(&self, ctx: &Context) -> bool {
        ctx.config.packages.iter().any(|p| !p.is_aur)
    }
    fn run(&self, ctx: &Context) -> Result<TaskResult> {
        if ctx.platform.is_linux() { install_pacman(ctx, &packages) }
        else { install_winget(ctx, &packages) }
    }
}
```

### `InstallParu` — AUR helper bootstrap
Runs only on Arch when `paru` is missing. Clones `paru-bin` from AUR and builds with `makepkg`.

### `InstallAurPackages` — paru
Runs only on Arch with `paru` installed. Uses `paru -S --needed --noconfirm`.

## Implementation Patterns

### Linux (pacman)
```rust
fn install_pacman(ctx: &Context, packages: &[&Package]) -> Result<TaskResult> {
    if !exec::which("pacman") {
        return Ok(TaskResult::Skipped("pacman not found".to_string()));
    }
    let names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    if ctx.dry_run {
        ctx.log.dry_run(&format!("sudo pacman -S --needed --noconfirm {}", names.join(" ")));
        return Ok(TaskResult::DryRun);
    }
    exec::run("sudo", &["pacman", "-S", "--needed", "--noconfirm", ...])?;
    Ok(TaskResult::Ok)
}
```

### Windows (winget)
Iterates packages individually with `winget install --id <id> --exact`. Uses `exec::run_unchecked` since winget returns non-zero for already-installed packages.

## Key Patterns

- **Batch install**: Collect all package names and pass to a single pacman call
- **Idempotent**: `--needed` flag skips already-installed packages
- **Guard with `exec::which`**: Skip gracefully if package manager not found
- **No sudo for paru**: paru manages elevation internally
- **`run_unchecked` for winget**: Allows handling non-zero exits without failing the task

## Rules

1. Always check `exec::which("pacman")` / `exec::which("winget")` before calling
2. Use `--needed --noconfirm` for pacman to ensure idempotency
3. AUR packages must be in `[*,aur]` sections to be tagged correctly
4. Use exact package IDs on Windows (case-sensitive)
5. Return `TaskResult::Skipped` with reason when package manager is missing
