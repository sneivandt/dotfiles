# Windows Usage

Opinionated Windows automation layer for this dotfiles project. The PowerShell entrypoint wires together registry personalization, fonts, symlinks, and VS Code extensions in an idempotent fashion with profile-based filtering.

## Quick Start

Open an elevated PowerShell (most tasks require admin to write HKCU console keys, install fonts, and create symlinks in some protected locations) then run:

```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.ps1
```

Re‑run the script at any time; operations are skipped when already satisfied (fonts present, extensions installed, registry values unchanged, symlinks existing).

## What the Script Does

`dotfiles.ps1` loads each module under `src/` and executes these functions in order:

| Step | Module | Function | Description | Idempotency Cue |
|------|--------|----------|-------------|-----------------|
| 1 | `Git.psm1` | `Update-GitSubmodules` | Initializes / updates all tracked submodules (fonts, vim plugins). | Only runs `git submodule update` if status indicates drift (`+` / `-`). |
| 2 | `Registry.psm1` | `Sync-Registry` | Applies registry values from `conf/registry.ini` filtered by profile. | Each value compared to existing; paths created only if missing. |
| 3 | `Font.psm1` | `Install-Fonts` | Installs the Powerline patched font (`DejaVu Sans Mono for Powerline`). | Skips if font already exists in system or per-user font directory. |
| 4 | `Symlinks.psm1` | `Install-Symlinks` | Creates Windows user profile symlinks from `conf/symlinks.ini` filtered by profile. | Only creates links whose targets do not already exist. |
| 5 | `VsCodeExtensions.psm1` | `Install-VsCodeExtensions` | Ensures VS Code extensions listed in `conf/vscode-extensions.ini` are installed. | Checks against `code --list-extensions`. |

## Registry Customization

Registry configuration lives in `conf/registry.ini` using INI format with **registry paths as sections**:

```ini
[HKCU:\Console\PSReadLine]
NormalForeground = 0xF

[HKCU:\Control Panel\International]
sLongDate = MMMM d, yyyy
```

Each section header is a registry path, and entries use `name = value` format. Color table entries (ColorTable00-15) use 6-digit hex RGB format that gets automatically converted to the internal DWORD format.

**Note:** Registry configuration doesn't use profile filtering since registry settings are Windows-only by nature.

To add custom registry settings:
1. Add entries to appropriate section in `conf/registry.ini` (or create a new section with a registry path).
2. Re-run `./dotfiles.ps1`.

The script will create missing registry keys automatically.

## Symlinks

Symlink definitions live in `conf/symlinks.ini` under the **`[windows]` section**:

```ini
[windows]
AppData/Roaming/Code/User/settings.json
config/git/config
config/powershell/Microsoft.PowerShell_profile.ps1
```

Each line is a path relative to `$env:USERPROFILE`. The source file is located at `symlinks/<same-path>` in the repository. Forward slashes are automatically converted to backslashes for Windows.

**Note:** Windows symlinks now share the same configuration file as Linux (`conf/symlinks.ini`) but use the `[windows]` section.

To add a new link:
1. Place the source file under `symlinks/<path>` (create directories as needed).
2. Add the path to the `[windows]` section in `conf/symlinks.ini`.
3. Re-run `./dotfiles.ps1`.

## VS Code Extensions

The file `conf/vscode-extensions.ini` contains extensions in the `[extensions]` section. Remove a line and re-run to keep new installs from occurring (does not uninstall). Add lines to expand your standard environment. The script requires the `code` CLI on PATH (Enable via VS Code: Command Palette → Shell Command: Install 'code' command in PATH).

## Fonts

Font installation delegates to `extern/fonts/install.ps1` (a git submodule from powerline/fonts repository). Currently only ensures the Powerline patched DejaVu Sans Mono. The submodule is automatically updated when running `./dotfiles.ps1`.

## Updating

Pull latest changes then re-run:

```powershell
git pull
./dotfiles.ps1
```

All submodules are checked and updated automatically when the script runs.

## Troubleshooting

| Symptom | Check |
|---------|-------|
| No output / nothing changes | Ensure you are running an elevated PowerShell session. |
| Symlink not created | Entry present in `conf/symlinks.ini` under `[windows]` section? Does source file exist in `symlinks/`? Does a real file already exist at target path (preventing link)? |
| Registry values unchanged | Verify keys under `HKCU:\Console` – did policy or another tool override them? Run as admin. |
| Font not applied in terminal | Confirm the terminal profile is set to the installed font manually (script installs, but doesn't change terminal profile). |
| VS Code extensions not installing | `code` CLI available? Run `code --version` in the same session. |

## Safety & Idempotency Notes

* Script does not delete existing regular files that block symlink creation; you'll need to back them up and remove manually.
* Registry writes are limited to HKCU (user scope) console keys and additional configured paths; no HKLM modifications occur.
* Re-running is safe; modules emit section headers only when performing actions.

## Extending Windows Layer

1. Create or modify module in `src/` exporting a function.
2. Add its invocation to `dotfiles.ps1` (maintain logical ordering: core prerequisites first, leaf operations last).
3. Keep functions self‑guarded (no-op if already configured) to preserve idempotency.