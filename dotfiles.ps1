<#
.SYNOPSIS
    Windows bootstrap entry point for dotfiles repository.
.DESCRIPTION
    This script is a thin wrapper around the Dotfiles PowerShell module.
    It loads the module and calls Install-Dotfiles to perform a full setup:
      * Git configuration (Initialize-GitConfig)
      * Repository update (Update-DotfilesRepository)
      * Git hooks installation (Install-RepositoryGitHooks)
      * Dotfiles module installation (Install-DotfilesModule)
      * Package installation (Install-Packages)
      * Registry configuration (Sync-Registry / conf/registry.ini)
      * Symlink creation (Install-Symlinks)
      * VS Code Extensions (Install-VsCodeExtensions)

    Registry operations and symlink creation require administrator privileges
    when not in dry-run mode.

    The script always uses the "windows" profile. Profile selection is not
    supported on Windows.
.PARAMETER DryRun
    When specified, logs all actions that would be taken without making
    system modifications. Use -Verbose for detailed output.
.NOTES
    Compatible with both PowerShell Core (pwsh) and Windows PowerShell (5.1+)
    Admin: Required for registry modification and symlink creation
           (not required in dry-run mode)
.EXAMPLE
    PS> .\dotfiles.ps1
    Executes complete provisioning sequence with "windows" profile.
.EXAMPLE
    PS> .\dotfiles.ps1 -DryRun
    Show what would be changed without making modifications.
.EXAMPLE
    PS> .\dotfiles.ps1 -Verbose
    Show detailed installation progress.
#>

[CmdletBinding()]
param (
    [Parameter(Mandatory = $false)]
    [switch]
    $DryRun
)

# Import the Dotfiles module from the repository
$modulePath = Join-Path $PSScriptRoot "src\windows\Dotfiles.psm1"

try
{
    Import-Module $modulePath -Force -ErrorAction Stop

    # Call Install-Dotfiles with the same parameters
    Install-Dotfiles -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')
}
catch
{
    Write-Error "Failed to run dotfiles installation: $_"
    exit 1
}
