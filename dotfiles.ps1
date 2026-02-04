#Requires -PSEdition Core

<#
.SYNOPSIS
    Windows bootstrap entry point for dotfiles repository.
.DESCRIPTION
    Aggregates module functions from src/ and performs a full setup:
      * Git configuration (Initialize-GitConfig)
      * Registry configuration (Sync-Registry / conf/registry.ini)
      * Symlink creation (Install-Symlinks)
      * VS Code Extensions (Install-VsCodeExtensions)

    Registry operations and symlink creation require administrator privileges
    when not in dry-run mode. Script is intentionally linear; each function
    internally guards idempotency to allow safe re-runs.

    The script always uses the "windows" profile. Profile selection is not
    supported on Windows.
.PARAMETER DryRun
    When specified, logs all actions that would be taken without making
    system modifications. Use -Verbose for detailed output.
.NOTES
    Requires: PowerShell Core
    Admin: Required for registry modification and symlink creation
           (not required in dry-run mode)
.EXAMPLE
    PS> .\dotfiles.ps1
    Executes complete provisioning sequence with "windows" profile.
.EXAMPLE
    PS> .\dotfiles.ps1 -DryRun
    Show what would be changed without making modifications.
#>

[CmdletBinding()]
param (
    [Parameter(Mandatory = $false)]
    [switch]
    $DryRun
)

# Check for administrator privileges when not in dry-run mode
if (-not $DryRun)
{
    # Only check for admin on Windows (this script is Windows-only anyway)
    if ($IsWindows -or (-not (Get-Variable -Name IsWindows -ErrorAction SilentlyContinue)))
    {
        try
        {
            $currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
            $isAdmin = $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
            if (-not $isAdmin)
            {
                Write-Error "This script requires administrator privileges to modify registry settings and create symlinks. Please run as administrator or use -DryRun to preview changes."
                exit 1
            }
        }
        catch
        {
            # If we can't determine admin status (e.g., on non-Windows), assume it's okay
            Write-Verbose "Unable to determine administrator status: $($_.Exception.Message)"
        }
    }
}

# Windows always uses the "windows" profile
$SelectedProfile = "windows"

foreach ($module in Get-ChildItem $PSScriptRoot\src\windows\*.psm1)
{
    # Import each supporting module (Profile, Registry, Symlinks, VsCodeExtensions)
    # -Force ensures updated definitions override any cached versions when re-run.
    Import-Module $module.FullName -Force
}

if ($DryRun)
{
    Write-Output ":: DRY-RUN MODE: No system modifications will be made"
}

# Get excluded categories for this profile
$excluded = Get-ProfileExclusion -Root $PSScriptRoot -ProfileName $SelectedProfile

Initialize-GitConfig -Root $PSScriptRoot -DryRun:$DryRun
Install-RepositoryGitHooks -root $PSScriptRoot -DryRun:$DryRun
Sync-Registry -root $PSScriptRoot -DryRun:$DryRun
Install-Symlinks -root $PSScriptRoot -excludedCategories $excluded -DryRun:$DryRun
Install-VsCodeExtensions -root $PSScriptRoot -excludedCategories $excluded -DryRun:$DryRun
