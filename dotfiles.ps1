#Requires -PSEdition Desktop
#Requires -RunAsAdministrator

<#
.SYNOPSIS
    Windows bootstrap entry point for dotfiles repository.
.DESCRIPTION
    Aggregates module functions from src/ and performs a full setup:
      * Git submodule sync (Update-GitSubmodules)
      * Registry configuration (Sync-Registry / conf/registry.ini)
      * Font installation (Install-Fonts)
      * Symlink creation (Install-Symlinks)
      * VS Code Extensions (Install-VsCodeExtensions)

    Must run elevated for registry + font operations. Script is intentionally
    linear; each function internally guards idempotency to allow safe re-runs.

    The script always uses the "windows" profile. Profile selection is not
    supported on Windows.
.PARAMETER DryRun
    When specified, logs all actions that would be taken without making
    system modifications. Verbose output is automatically enabled in dry-run
    mode to provide detailed visibility into intended actions.
.NOTES
    Keep this file minimal—logic lives in imported modules for testability.
.EXAMPLE
    PS> .\dotfiles.ps1
    Executes complete provisioning sequence with "windows" profile.
.EXAMPLE
    PS> .\dotfiles.ps1 -DryRun
    Show what would be changed without making modifications (verbose auto-enabled).
#>

[CmdletBinding()]
param (
    [Parameter(Mandatory = $false)]
    [switch]
    $DryRun
)

# Windows always uses the "windows" profile
$SelectedProfile = "windows"

# Automatically enable verbose output when in dry-run mode
if ($DryRun)
{
    $VerbosePreference = 'Continue'
}

foreach ($module in Get-ChildItem $PSScriptRoot\src\windows\*.psm1)
{
    # Import each supporting module (Font, Git, Profile, Registry, Symlinks, VsCodeExtensions)
    # -Force ensures updated definitions override any cached versions when re-run.
    Import-Module $module.FullName -Force
}

Write-Output ":: Using profile: $SelectedProfile"
if ($DryRun)
{
    Write-Output ":: DRY-RUN MODE: No system modifications will be made"
}

# Get excluded categories for this profile
$excluded = Get-ProfileExclusion -Root $PSScriptRoot -ProfileName $SelectedProfile

Update-GitSubmodules -root $PSScriptRoot -DryRun:$DryRun
# Note: Registry settings are not profile-filtered (Windows-only, applies to all)
Sync-Registry -root $PSScriptRoot -DryRun:$DryRun
Install-Fonts -root $PSScriptRoot -DryRun:$DryRun
Install-Symlinks -root $PSScriptRoot -excludedCategories $excluded -DryRun:$DryRun
Install-VsCodeExtensions -root $PSScriptRoot -DryRun:$DryRun