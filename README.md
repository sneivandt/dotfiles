# Dotfiles ✨

Opinionated, scriptable, cross‑platform (Linux / Arch / Windows) dotfiles with:

- Declarative symlink definitions (text and JSON)
- Optional package + systemd unit installation
- Segmented environment layers (base, gui, arch, windows)
- Reproducible test mode + Docker image
- Editor (VS Code) & shell (zsh/bash) configuration

[![Publish Docker image](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml/badge.svg)](https://github.com/sneivandt/dotfiles/actions/workflows/docker-image.yml)

## Quick Start 🚀

Install base layer (shell, git, vim/nvim, etc.):
```bash
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.sh -I
```

Uninstall (remove managed symlinks / units):
```bash
./dotfiles.sh -U
```

Help:
```bash
./dotfiles.sh -h
```

## Usage Summary 🛠️

```
dotfiles.sh
dotfiles.sh {-I --install}   [-g] [-p] [-s] [-v]
dotfiles.sh {-U --uninstall} [-g] [-v]
dotfiles.sh {-T --test}      [-v]
dotfiles.sh {-h --help}

Options:
  -g  Include GUI environment layer
  -p  Install system packages defined for the layer
  -s  Install systemd user units for the layer
  -v  Enable verbose logging
```

## Layered Environments (`env/`) 🧩

Each directory under `env/` encapsulates a logical layer. Layers can extend one another (e.g. `arch-gui` builds on `arch`, `base-gui` builds on `base`).

| Layer | Purpose |
|-------|---------|
| `base` | Cross‑platform core shell + editor + git + tooling configs |
| `base-gui` | GUI/editor (VS Code, JetBrains placeholder dirs, etc.) extras |
| `arch` | Arch Linux specific packages & pacman configuration |
| `arch-gui` | Arch desktop (X, xmonad, picom, dunst, redshift, fonts) |
| `win` | Windows / PowerShell / registry settings & symlink metadata |

### Key Layer Files

| File | Description |
|------|-------------|
| `symlinks.conf` / `symlinks.json` | Declarative list of source → target mappings that `dotfiles.sh` materializes |
| `packages.conf` | Plain list of packages (pacman / AUR or other package managers as implied) |
| `units.conf` | Systemd user units to enable/link |
| `chmod.conf` | Post‑install permission adjustments |
| `submodules.conf` | Git submodules to init / update |

Symlink source files live under `symlinks/` within each layer. The script resolves and links them into `$HOME` (and sometimes nested config directories) while preserving pre‑existing files by backing them up (see Implementation notes in script – if not currently backing up, consider adding before destructive operations).

## Scripts (`./dotfiles.sh`) 📜

Primary entrypoint: `dotfiles.sh`

Supporting shell utilities reside in `src/` (e.g. `commands.sh`, `logger.sh`, `utils.sh`, `tasks.sh`) providing:
* Logging abstraction
* Idempotent symlink creation
* Layer resolution / ordering
* Package + unit install helpers

PowerShell module for Windows lives in `src/script.psm1` with supporting modules under `win/src/` for registry, symlinks, VS Code extensions, etc.

### Windows

See `WINDOWS.md` and the `win/` directory for:
* Registry presets (`registry.json`, `registry-shell.json`)
* PowerShell profile (`Microsoft.PowerShell_profile.ps1` under `env/base/symlinks/...`)
* VS Code extension management logic (`VsCodeExtensions.psm1`)

Usage pattern (PowerShell, elevated as required):
```powershell
.\dotfiles.ps1
```

## Docker 🐳

Run the published image for an isolated test shell (non‑destructive):
```bash
docker run --rm -it sneivandt/dotfiles
```

This image is built by the included GitHub Actions workflow (`docker-image.yml`). Useful for quickly validating scripts on a clean base environment.

## Customization ✨

1. Fork the repo (recommended) or create a feature branch.
2. Add or modify files under the appropriate layer `symlinks/` tree.
3. Update `symlinks.conf` (or `.json`) with new mappings.
4. Add packages to `packages.conf` (one per line).
5. Add / adjust systemd units in `units.conf` and place unit files under `symlinks/config/systemd/user/`.
6. Test with `./dotfiles.sh -T` before a full install.

### Adding a New Layer
* Create `env/<name>/` with at least a `symlinks.conf` (even if empty) and `README.md` describing its purpose.
* Ensure layer ordering logic (if hard‑coded) recognizes it; if dynamic, naming alone may suffice.

## Troubleshooting 🔍

| Symptom | Check |
|---------|-------|
| Symlink not created | Entry missing in layer's `symlinks.conf`? Conflicting existing file? Permissions? |
| Package not installed | Present in correct `packages.conf` for selected flags? Package manager available? |
| Systemd unit inactive | Was `-s` passed? Verify with `systemctl --user status <unit>` |
| Windows registry not applied | Run PowerShell as admin; confirm `Registry.psm1` imported without errors |
