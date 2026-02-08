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

        The module is installed to the first existing location under $env:USERPROFILE
        that is listed in $env:PSModulePath, typically
        %USERPROFILE%\Documents\PowerShell\Modules (PowerShell Core) or
        %USERPROFILE%\Documents\WindowsPowerShell\Modules (Windows PowerShell).

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

    # Determine target module directory
    $userModulePaths = $env:PSModulePath -split [System.IO.Path]::PathSeparator |
        Where-Object { $_ -like "*$env:USERPROFILE*" } |
        Where-Object { Test-Path $_ -PathType Container -ErrorAction SilentlyContinue }

    if (-not $userModulePaths)
    {
        # No existing user module path found, create default one
        $psVersion = $PSVersionTable.PSVersion.Major
        if ($psVersion -ge 6)
        {
            # PowerShell Core
            $targetBase = Join-Path $env:USERPROFILE "Documents\PowerShell\Modules"
        }
        else
        {
            # Windows PowerShell
            $targetBase = Join-Path $env:USERPROFILE "Documents\WindowsPowerShell\Modules"
        }

        if (-not (Test-Path $targetBase))
        {
            if ($DryRun)
            {
                Write-Verbose "Would create module directory: $targetBase"
            }
            else
            {
                Write-Verbose "Creating module directory: $targetBase"
                New-Item -ItemType Directory -Path $targetBase -Force | Out-Null
            }
        }

        $targetModuleDir = Join-Path $targetBase "Dotfiles"
    }
    else
    {
        # Use first writable user module path
        $targetBase = $userModulePaths[0]
        $targetModuleDir = Join-Path $targetBase "Dotfiles"
        Write-Verbose "Target module path: $targetBase"
    }

    # Check if module already installed
    $existingModule = Get-Module -ListAvailable -Name Dotfiles -ErrorAction SilentlyContinue |
        Where-Object { $_.ModuleBase -eq $targetModuleDir } |
        Select-Object -First 1

    if ($existingModule)
    {
        Write-Verbose "Dotfiles module already installed at: $targetModuleDir"
        Write-Verbose "Existing version: $($existingModule.Version)"
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
        Write-Verbose "Skipping module installation: already up to date"
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
            Write-Verbose "Creating module directory: $targetModuleDir"
            New-Item -ItemType Directory -Path $targetModuleDir -Force | Out-Null
        }

        # Copy module files
        foreach ($file in $filesToCopy)
        {
            $sourcePath = Join-Path $Root $file
            $targetPath = Join-Path $targetModuleDir $file

            if (Test-Path $sourcePath)
            {
                Write-Verbose "Copying: $file"
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
                Write-Verbose "Copying directory: $dir"
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
