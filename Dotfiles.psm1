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

# Store the module root for accessing repository files
$Script:ModuleRoot = $PSScriptRoot

[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Dotfiles is a product name, not a plural')]
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
                    Write-Error "This command requires administrator privileges to modify registry settings and create symlinks. Please run as administrator or use -DryRun to preview changes."
                    return
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

    Write-Verbose "Loading Windows modules from: $Script:ModuleRoot\src\windows\"
    foreach ($module in Get-ChildItem "$Script:ModuleRoot\src\windows\*.psm1")
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
    $excluded = Get-ProfileExclusion -Root $Script:ModuleRoot -ProfileName $SelectedProfile
    Write-Verbose "Excluded categories: $(if ($excluded) { $excluded } else { '(none)' })"

    Write-Verbose "Starting installation sequence..."

    Write-Verbose "[1/7] Initializing Git configuration..."
    Initialize-GitConfig -Root $Script:ModuleRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-Verbose "[2/7] Checking for repository updates..."
    Update-DotfilesRepository -Root $Script:ModuleRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-Verbose "[3/7] Installing repository git hooks..."
    Install-RepositoryGitHooks -Root $Script:ModuleRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    # Note: Module installation step is skipped when running from the module itself
    # (dotfiles.ps1 includes this as step 4/8)

    Write-Verbose "[4/7] Installing packages..."
    Install-Packages -Root $Script:ModuleRoot -ExcludedCategories $excluded -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-Verbose "[5/7] Syncing registry settings..."
    Sync-Registry -Root $Script:ModuleRoot -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-Verbose "[6/7] Installing symlinks..."
    Install-Symlinks -Root $Script:ModuleRoot -ExcludedCategories $excluded -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-Verbose "[7/7] Installing VS Code extensions..."
    Install-VsCodeExtensions -Root $Script:ModuleRoot -ExcludedCategories $excluded -DryRun:$DryRun -Verbose:($VerbosePreference -eq 'Continue')

    Write-Verbose "Installation sequence complete!"
}

[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Dotfiles is a product name, not a plural')]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Delegates to Install-Dotfiles which handles all state changes')]
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
