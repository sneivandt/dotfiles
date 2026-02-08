<#
.SYNOPSIS
    Module installation utilities for Windows dotfiles
.DESCRIPTION
    Provides functions to install the Dotfiles module into the user's
    PowerShell modules path, making it available as a command anywhere.
.NOTES
    Admin: Not required
#>

function Install-DotfilesModule
{
    <#
    .SYNOPSIS
        Install Dotfiles module to user's PowerShell modules directory
    .DESCRIPTION
        Copies the Dotfiles module files to the user's PowerShell modules
        directory, making the Install-Dotfiles and Update-Dotfiles commands
        available from anywhere in PowerShell.

        The module is installed to the first user-scoped directory in $env:PSModulePath
        (typically %USERPROFILE%\Documents\PowerShell\Modules\Dotfiles for PowerShell Core
        or %USERPROFILE%\Documents\WindowsPowerShell\Modules\Dotfiles for Windows PowerShell).

        After installation, you can run:
            Install-Dotfiles        # Install/update dotfiles
            Update-Dotfiles         # Update repository and re-install
            Get-Help Install-Dotfiles -Full  # View help
    .PARAMETER Root
        Root directory of the dotfiles repository
    .PARAMETER DryRun
        If specified, shows what would be done without making changes
    .EXAMPLE
        Install-DotfilesModule -Root $PSScriptRoot
        Installs the module to the user's modules directory
    .EXAMPLE
        Install-DotfilesModule -Root $PSScriptRoot -DryRun
        Shows where the module would be installed
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Root,

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    # Track if we've printed the stage header
    $act = $false

    # Find the first writable user modules directory in PSModulePath
    $modulePaths = $env:PSModulePath -split [System.IO.Path]::PathSeparator
    $userModulePath = $null

    foreach ($path in $modulePaths)
    {
        # Look for paths under USERPROFILE (not Program Files or system directories)
        if ($path -like "$env:USERPROFILE*")
        {
            $userModulePath = $path
            break
        }
    }

    # Fallback to default if not found
    if (-not $userModulePath)
    {
        # Use the standard user module path for current PowerShell edition
        if ($PSVersionTable.PSEdition -eq 'Core')
        {
            $userModulePath = Join-Path $env:USERPROFILE "Documents\PowerShell\Modules"
        }
        else
        {
            # Desktop edition (Windows PowerShell)
            $userModulePath = Join-Path $env:USERPROFILE "Documents\WindowsPowerShell\Modules"
        }
    }

    # Ensure the modules directory exists
    if (-not (Test-Path $userModulePath))
    {
        Write-VerboseMessage "Creating user modules directory: $userModulePath"
        if (-not $DryRun)
        {
            New-Item -ItemType Directory -Path $userModulePath -Force | Out-Null
        }
    }

    # Target module directory
    $targetModuleDir = Join-Path $userModulePath "Dotfiles"
    Write-VerboseMessage "Target module path: $targetModuleDir"

    # Check if module already installed
    $existingModule = Get-Module -ListAvailable -Name Dotfiles -ErrorAction SilentlyContinue |
        Where-Object { $_.ModuleBase -eq $targetModuleDir } |
        Select-Object -First 1

    if ($existingModule)
    {
        Write-VerboseMessage "Dotfiles module already installed at: $targetModuleDir"
        Write-VerboseMessage "Existing version: $($existingModule.Version)"
    }

    # Files to copy
    $filesToCopy = @(
        "Dotfiles.psd1",
        "Dotfiles.psm1"
    )

    # Directories to copy
    $dirsToCopy = @(
        "src",
        "conf",
        "symlinks"
    )

    # Check if we need to update
    $needsUpdate = $false
    if (-not $existingModule)
    {
        $needsUpdate = $true
    }
    else
    {
        # Check if any files are different
        foreach ($file in $filesToCopy)
        {
            $sourcePath = Join-Path $Root $file
            $targetPath = Join-Path $targetModuleDir $file

            if (-not (Test-Path $targetPath))
            {
                $needsUpdate = $true
                break
            }

            $sourceHash = (Get-FileHash -Path $sourcePath -Algorithm SHA256).Hash
            $targetHash = (Get-FileHash -Path $targetPath -Algorithm SHA256).Hash

            if ($sourceHash -ne $targetHash)
            {
                $needsUpdate = $true
                break
            }
        }

        # Check if any directories need updating (check existence and do a simple comparison)
        if (-not $needsUpdate)
        {
            foreach ($dir in $dirsToCopy)
            {
                $sourcePath = Join-Path $Root $dir
                $targetPath = Join-Path $targetModuleDir $dir

                if (-not (Test-Path $targetPath))
                {
                    $needsUpdate = $true
                    break
                }

                # Simple check: compare file counts as a heuristic
                $sourceFiles = Get-ChildItem -Path $sourcePath -Recurse -File -ErrorAction SilentlyContinue
                $targetFiles = Get-ChildItem -Path $targetPath -Recurse -File -ErrorAction SilentlyContinue

                if ($sourceFiles.Count -ne $targetFiles.Count)
                {
                    $needsUpdate = $true
                    break
                }
            }
        }
    }

    if (-not $needsUpdate)
    {
        Write-VerboseMessage "Skipping module installation: already up to date"
        Write-VerboseMessage "Module location: $targetModuleDir"
        return
    }

    # Install/update the module
    if (-not $act)
    {
        $act = $true
        Write-Output ":: Installing Dotfiles Module"
    }

    if ($DryRun)
    {
        Write-Output "DRY-RUN: Would install Dotfiles module to: $targetModuleDir"
        Write-Output "DRY-RUN: Would copy files: $($filesToCopy -join ', ')"
        Write-Output "DRY-RUN: Would copy directories: $($dirsToCopy -join ', ')"
    }
    else
    {
        # Create target directory
        if (-not (Test-Path $targetModuleDir))
        {
            Write-VerboseMessage "Creating module directory: $targetModuleDir"
            New-Item -ItemType Directory -Path $targetModuleDir -Force | Out-Null
        }

        # Copy module files
        foreach ($file in $filesToCopy)
        {
            $sourcePath = Join-Path $Root $file
            $targetPath = Join-Path $targetModuleDir $file

            if (Test-Path $sourcePath)
            {
                Write-VerboseMessage "Copying: $file"
                Copy-Item -Path $sourcePath -Destination $targetPath -Force
            }
            else
            {
                Write-Warning "Source file not found: $sourcePath"
            }
        }

        # Copy directories
        foreach ($dir in $dirsToCopy)
        {
            $sourcePath = Join-Path $Root $dir
            $targetPath = Join-Path $targetModuleDir $dir

            if (Test-Path $sourcePath)
            {
                Write-VerboseMessage "Copying directory: $dir"
                # Remove existing directory if present
                if (Test-Path $targetPath)
                {
                    Remove-Item -Path $targetPath -Recurse -Force
                }
                Copy-Item -Path $sourcePath -Destination $targetPath -Recurse -Force
            }
            else
            {
                Write-Warning "Source directory not found: $sourcePath"
            }
        }

        Write-Output "Dotfiles module installed to: $targetModuleDir"
        Write-Output ""
        Write-Output "The following commands are now available:"
        Write-Output "  Install-Dotfiles    - Install or update dotfiles configuration"
        Write-Output "  Update-Dotfiles     - Update repository and re-install"
        Write-Output ""
        Write-Output "Example usage:"
        Write-Output "  Install-Dotfiles"
        Write-Output "  Install-Dotfiles -DryRun -Verbose"
        Write-Output "  Update-Dotfiles"
        Write-Output "  Get-Help Install-Dotfiles -Full"
        Write-Output ""
        Write-Output "To reload the module in the current session, run:"
        Write-Output "  Import-Module Dotfiles -Force"
    }
}

Export-ModuleMember -Function Install-DotfilesModule
