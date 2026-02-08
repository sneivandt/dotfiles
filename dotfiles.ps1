<#
.SYNOPSIS
    Windows bootstrap entry point for dotfiles repository.
.DESCRIPTION
    Aggregates module functions from src/ and performs a full setup:
      * Git configuration (Initialize-GitConfig)
      * Package installation (Install-Packages)
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
    Compatible with both PowerShell Core (pwsh) and Windows PowerShell (5.1+)
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
                # Automatically request elevation by re-launching the script
                Write-Output "Not running as administrator. Requesting elevation..."

                # Determine which PowerShell executable to use
                $edition = $PSVersionTable.PSEdition
                if ($edition -eq 'Core')
                {
                    $psExe = 'pwsh'
                }
                else
                {
                    # Desktop edition (Windows PowerShell)
                    $psExe = 'powershell'
                }

                # Build argument list
                $arguments = @(
                    '-NoProfile',
                    '-ExecutionPolicy', 'Bypass',
                    '-File', ('"{0}"' -f $PSCommandPath)
                )

                # Preserve -Verbose flag
                if ($VerbosePreference -eq 'Continue')
                {
                    $arguments += '-Verbose'
                }

                # Launch elevated process
                try
                {
                    $process = Start-Process -FilePath $psExe -ArgumentList $arguments -Verb RunAs -PassThru -Wait
                    # Check if elevated process failed
                    if ($process.ExitCode -ne 0)
                    {
                        Write-Error "Elevated process failed with exit code $($process.ExitCode). Run with -Verbose for detailed output, or use -DryRun to preview changes."
                    }
                    # Return the exit code from the elevated process
                    exit $process.ExitCode
                }
                catch [System.ComponentModel.Win32Exception]
                {
                    # UAC was cancelled (ERROR_CANCELLED = 1223) or access denied
                    if ($_.Exception.NativeErrorCode -eq 1223)
                    {
                        Write-Error "UAC elevation was cancelled by user. Administrator privileges are required to modify registry settings and create symlinks. Please run as administrator or use -DryRun to preview changes."
                    }
                    else
                    {
                        Write-Error "Failed to elevate: $($_.Exception.Message)"
                    }
                    exit 1
                }
                catch
                {
                    # Other unexpected errors (e.g., PowerShell executable not found)
                    Write-Error "Failed to start elevated process: $($_.Exception.Message)"
                    exit 1
                }
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
Write-Verbose "Using profile: $SelectedProfile"

Write-Verbose "Loading Windows modules from: src/windows/"
foreach ($module in Get-ChildItem $PSScriptRoot\src\windows\*.psm1)
{
    # Import each supporting module (Profile, Registry, Symlinks, VsCodeExtensions)
    # -Force ensures updated definitions override any cached versions when re-run.
    Write-Verbose "Importing module: $($module.Name)"
    Import-Module $module.FullName -Force
}

if ($DryRun)
{
    Write-Output ":: DRY-RUN MODE: No system modifications will be made"
}

# Get excluded categories for this profile
Write-Verbose "Resolving excluded categories for profile: $SelectedProfile"
$excluded = Get-ProfileExclusion -Root $PSScriptRoot -ProfileName $SelectedProfile
Write-Verbose "Excluded categories: $(if ($excluded) { $excluded } else { '(none)' })"

Write-Verbose "Starting installation sequence..."

Write-Verbose "[1/7] Initializing Git configuration..."
Initialize-GitConfig -Root $PSScriptRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

Write-Verbose "[2/7] Checking for repository updates..."
Update-DotfilesRepository -Root $PSScriptRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

Write-Verbose "[3/7] Installing repository git hooks..."
Install-RepositoryGitHooks -Root $PSScriptRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

Write-Verbose "[4/7] Installing packages..."
Install-Packages -Root $PSScriptRoot -ExcludedCategories $excluded -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

Write-Verbose "[5/7] Syncing registry settings..."
Sync-Registry -Root $PSScriptRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

Write-Verbose "[6/7] Installing symlinks..."
Install-Symlinks -Root $PSScriptRoot -ExcludedCategories $excluded -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

Write-Verbose "[7/7] Installing VS Code extensions..."
Install-VsCodeExtensions -Root $PSScriptRoot -ExcludedCategories $excluded -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

Write-Verbose "Installation sequence complete!"

# Ensure clean exit with success code
exit 0
