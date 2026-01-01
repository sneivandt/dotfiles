#Requires -PSEdition Desktop
#Requires -RunAsAdministrator

<#
.SYNOPSIS
    Windows bootstrap entry point for dotfiles repository.
.DESCRIPTION
    Aggregates module functions from env/win/src and performs a full setup:
      * Git submodule sync (Update-GitSubmodules)
      * Registry configuration (Sync-Registry / registry*.json)
      * Font installation (Install-Fonts)
      * Symlink creation (Install-Symlinks)
      * VS Code Extensions (Install-VsCodeExtensions)

    Must run elevated for registry + font operations. Script is intentionally
    linear; each function internally guards idempotency to allow safe re-runs.
.NOTES
    Keep this file minimal—logic lives in imported modules for testability.
.EXAMPLE
    PS> .\dotfiles.ps1
    Executes complete provisioning sequence.
#>

foreach ($module in Get-ChildItem $PSScriptRoot\env\win\src\*.psm1)
{
    # Import each supporting module (Font, Git, Registry, Symlinks, VsCodeExtensions)
    # -Force ensures updated definitions override any cached versions when re-run.
    Import-Module $module.FullName -Force
}

Update-GitSubmodules $PSScriptRoot      # Ensure nested git content up to date

Sync-Registry $PSScriptRoot             # Apply registry tweaks declaratively

Install-Fonts $PSScriptRoot             # Install missing fonts (per-user or system)
Install-Symlinks $PSScriptRoot          # Create/update Windows symlinks
Install-VsCodeExtensions $PSScriptRoot  # Ensure VS Code extensions installed