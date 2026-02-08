<#
.SYNOPSIS
    Dotfiles PowerShell module for Windows
.DESCRIPTION
    Provides commands to install and update dotfiles configuration on Windows.
    This module wraps the dotfiles.ps1 script functionality into reusable
    PowerShell commands that can be run from anywhere.
.NOTES
    Compatible with both PowerShell Core (pwsh) and Windows PowerShell (5.1+)
#>

function Get-DotfilesRoot
{
    <#
    .SYNOPSIS
        Resolve the root directory of the dotfiles repository
    .DESCRIPTION
        Prefer an explicit DOTFILES_ROOT environment variable when it points
        to a valid git repository. Otherwise, walk up from the module's
        install directory looking for a .git directory. If no repository
        can be found, fall back to the module root.
    #>

    # 1. Prefer an explicit environment variable if it points to a git repo
    $envRoot = $env:DOTFILES_ROOT
    if ($envRoot -and (Test-Path -LiteralPath $envRoot))
    {
        $gitDir = Join-Path -Path $envRoot -ChildPath '.git'
        if (Test-Path -LiteralPath $gitDir)
        {
            return (Resolve-Path -LiteralPath $envRoot).ProviderPath
        }
    }

    # 2. Walk up from the module directory to find the nearest .git
    $candidate = $PSScriptRoot
    while ($candidate)
    {
        $gitDir = Join-Path -Path $candidate -ChildPath '.git'
        if (Test-Path -LiteralPath $gitDir)
        {
            return $candidate
        }

        $parent = Split-Path -Path $candidate -Parent
        if (-not $parent -or $parent -eq $candidate)
        {
            break
        }

        $candidate = $parent
    }

    # 3. Fallback to the module install directory (previous behavior)
    return $PSScriptRoot
}

# Store the module root for accessing repository files
$Script:ModuleRoot = Get-DotfilesRoot

function Install-Dotfiles
{
    <#
    .SYNOPSIS
        Install dotfiles configuration on Windows
    .DESCRIPTION
        Performs a complete dotfiles installation including:
        - Git configuration
        - Repository update
        - Git hooks installation
        - Package installation (winget)
        - Registry configuration
        - Symlink creation
        - VS Code extensions installation

        Requires administrator privileges unless running in dry-run mode.
    .PARAMETER DryRun
        Preview changes without making system modifications
    .PARAMETER Verbose
        Show detailed output
    .EXAMPLE
        Install-Dotfiles
        Installs dotfiles with default settings
    .EXAMPLE
        Install-Dotfiles -DryRun -Verbose
        Preview what would be installed with detailed output
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Dotfiles is a product name, not a plural')]
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
                    # Automatically request elevation by re-launching the command
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

                    # Build command to re-run Install-Dotfiles in elevated session
                    $verboseArg = if ($VerbosePreference -eq 'Continue') { ' -Verbose' } else { '' }
                    $scriptCommand = @"
Import-Module Dotfiles -Force
Install-Dotfiles$verboseArg
Write-Host
Write-Host 'Installation complete. Press Enter to close...' -ForegroundColor Green
Read-Host
"@

                    $arguments = @(
                        '-NoProfile',
                        '-ExecutionPolicy', 'Bypass',
                        '-Command', $scriptCommand
                    )

                    # Launch elevated process without waiting
                    try
                    {
                        $process = Start-Process -FilePath $psExe -ArgumentList $arguments -Verb RunAs -PassThru
                        Write-Output "Elevated PowerShell window opened (PID: $($process.Id))."
                        # Exit original shell immediately
                        return
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
                        return
                    }
                    catch
                    {
                        # Other unexpected errors (e.g., PowerShell executable not found)
                        Write-Error "Failed to start elevated process: $($_.Exception.Message)"
                        return
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

    Write-Verbose "Loading Windows modules from: $Script:ModuleRoot\src\windows\"
    foreach ($module in Get-ChildItem "$Script:ModuleRoot\src\windows\*.psm1")
    {
        # Import each supporting module (Profile, Registry, Symlinks, VsCodeExtensions, Logging)
        # -Force ensures updated definitions override any cached versions when re-run.
        Write-Verbose "Importing module: $($module.Name)"
        Import-Module $module.FullName -Force
    }

    # Initialize logging system (log file, counters)
    Initialize-Logging -Profile $SelectedProfile

    Write-VerboseMessage "Using profile: $SelectedProfile"

    if ($DryRun)
    {
        Write-Stage -Message "DRY-RUN MODE: No system modifications will be made"
    }

    # Get excluded categories for this profile
    Write-VerboseMessage "Resolving excluded categories for profile: $SelectedProfile"
    $excluded = Get-ProfileExclusion -Root $Script:ModuleRoot -ProfileName $SelectedProfile
    Write-VerboseMessage "Excluded categories: $(if ($excluded) { $excluded } else { '(none)' })"

    Write-VerboseMessage "Starting installation sequence..."

    Write-VerboseMessage "[1/8] Initializing Git configuration..."
    Initialize-GitConfig -Root $Script:ModuleRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-VerboseMessage "[2/8] Checking for repository updates..."
    Update-DotfilesRepository -Root $Script:ModuleRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-VerboseMessage "[3/8] Installing repository git hooks..."
    Install-RepositoryGitHooks -Root $Script:ModuleRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-VerboseMessage "[4/8] Installing Dotfiles PowerShell module..."
    Install-DotfilesModule -Root $Script:ModuleRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-VerboseMessage "[5/8] Installing packages..."
    Install-Packages -Root $Script:ModuleRoot -ExcludedCategories $excluded -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-VerboseMessage "[6/8] Syncing registry settings..."
    Sync-Registry -Root $Script:ModuleRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-VerboseMessage "[7/8] Installing symlinks..."
    Install-Symlinks -Root $Script:ModuleRoot -ExcludedCategories $excluded -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-VerboseMessage "[8/8] Installing VS Code extensions..."
    Install-VsCodeExtensions -Root $Script:ModuleRoot -ExcludedCategories $excluded -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-VerboseMessage "Installation sequence complete!"

    # Display summary of operations
    Write-InstallationSummary -DryRun:$DryRun
}

function Update-Dotfiles
{
    <#
    .SYNOPSIS
        Update dotfiles repository and re-install configuration
    .DESCRIPTION
        Updates the dotfiles repository from the remote and re-runs the
        installation to apply any changes. Local changes are automatically
        stashed and re-applied.

        This is a convenience command that combines repository update with
        a fresh installation.
    .PARAMETER DryRun
        Preview changes without making system modifications
    .PARAMETER Verbose
        Show detailed output
    .EXAMPLE
        Update-Dotfiles
        Updates repository and re-installs dotfiles
    .EXAMPLE
        Update-Dotfiles -DryRun -Verbose
        Preview what would be updated with detailed output
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Dotfiles is a product name, not a plural')]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Delegates to Install-Dotfiles which handles all state changes')]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    Write-Output ":: Updating dotfiles repository and configuration"

    # Run the full installation, which includes the update step
    Install-Dotfiles -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')
}

Export-ModuleMember -Function Install-Dotfiles, Update-Dotfiles
