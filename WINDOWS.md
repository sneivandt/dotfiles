# Windows Usage 🪟

Opinionated Windows automation layer for this dotfiles project. The PowerShell entrypoint wires together registry personalization, fonts, symlinks, and VS Code extensions in an idempotent fashion.

## Quick Start

Open an elevated PowerShell (most tasks require admin to write HKCU console keys, install fonts, and create symlinks in some protected locations) then run:

```powershell
git clone https://github.com/sneivandt/dotfiles.git
cd dotfiles
./dotfiles.ps1
```

Re‑run the script at any time; operations are skipped when already satisfied (fonts present, extensions installed, registry values unchanged, symlinks existing).

## What the Script Does

`dotfiles.ps1` loads each module under `env/win/src/` and executes these functions in order:

| Step | Module | Function | Description | Idempotency Cue |
|------|--------|----------|-------------|-----------------|
| 1 | `Git.psm1` | `Update-GitSubmodules` | Initializes / updates tracked submodules needed for fonts & GUI config (`env/base-gui`, `env/win/fonts`). | Only runs `git submodule update` if status indicates drift (`+` / `-`). |
| 2 | `Registry.psm1` | `Sync-Registry` | Applies values from `registry.json` and console / shell color & behavior from `registry-shell.json` to a set of console keys. | Each value compared to existing; paths created only if missing. |
| 3 | `Font.psm1` | `Install-Fonts` | Installs the Powerline patched font (`DejaVu Sans Mono for Powerline`). | Skips if font already exists in system or per-user font directory. |
| 4 | `Symlinks.psm1` | `Install-Symlinks` | Creates Windows user profile symlinks defined in `env/win/symlinks.json`. | Only creates links whose targets do not already exist. |
| 5 | `VsCodeExtensions.psm1` | `Install-VsCodeExtensions` | Ensures VS Code extensions listed in `env/base-gui/vscode-extensions.conf` are installed. | Checks against `code --list-extensions`. |

## Registry Customization

Two JSON manifests drive registry changes:

* `registry.json` – Arbitrary path/name/value entries applied verbatim.
* `registry-shell.json` – Console host appearance (colors etc.). Color table entries are converted from RGB hex to the internal DWORD format.

Console keys targeted include default and PowerShell specific keys under `HKCU:\Console`. If you need to extend customization, add an entry to the JSON; the script will create missing keys.

## Symlinks

Symlink definitions live in `env/win/symlinks.json` with objects shaped:

```jsonc
[
	{ "Source": "win/symlinks/config/git/config", "Target": ".config/git/config" }
]
```

Source paths are resolved relative to the repository `env` directory; targets are relative to `$env:USERPROFILE`.

To add a new link:
1. Place the source file under `env/win/symlinks/...` (create directories as needed).
2. Add a JSON entry.
3. Re-run `./dotfiles.ps1`.

## VS Code Extensions

The file `env/base-gui/vscode-extensions.conf` is a simple newline list. Remove a line and re-run to keep new installs from occurring (does not uninstall). Add lines to expand your standard environment. The script requires the `code` CLI on PATH (Enable via VS Code: Command Palette → Shell Command: Install 'code' command in PATH).

## Fonts

Font installation delegates to `env/win/fonts/install.ps1` (a submodule). Currently only ensures the Powerline patched DejaVu Sans Mono. Add more logic there if you need additional fonts.

## Updating

Pull latest changes then re-run:

```powershell
./dotfiles.ps1
```

Submodules are only updated for the specific paths listed inside `Update-GitSubmodules`; extend that array to include new submodule-backed layers.

## Troubleshooting

| Symptom | Check |
|---------|-------|
| No output / nothing changes | Ensure you are running an elevated PowerShell session. |
| Symlink not created | Entry present in `symlinks.json`? Does a real file already exist at target path (preventing link)? |
| Registry values unchanged | Verify keys under `HKCU:\Console` – did policy or another tool override them? Run as admin. |
| Font not applied in terminal | Confirm the terminal profile is set to the installed font manually (script installs, but doesn't change terminal profile). |
| VS Code extensions not installing | `code` CLI available? Run `code --version` in the same session. |

## Safety & Idempotency Notes

* Script does not delete existing regular files that block symlink creation; you'll need to back them up and remove manually.
* Registry writes are limited to HKCU (user scope) console keys and additional configured paths; no HKLM modifications occur.
* Re-running is safe; modules emit section headers only when performing actions.

## Extending Windows Layer

1. Create or modify module in `env/win/src/` exporting a function.
2. Add its invocation to `dotfiles.ps1` (maintain logical ordering: core prerequisites first, leaf operations last).
3. Keep functions self‑guarded (no-op if already configured) to preserve idempotency.

## Related Layers

Although Windows specific logic lives here, shared configuration (shell, git, editor) originates from the `base` and `base-gui` layers via their symlinks when cloned / manually linked. At present the Windows script only manages items explicit to the Windows environment; cross‑platform symlinks are handled by invoking the Unix script on WSL or manually linking.