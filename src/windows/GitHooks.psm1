<#
.SYNOPSIS
    Git hooks management utilities for the dotfiles repository
.DESCRIPTION
    Installs git hooks for the dotfiles repository as symbolic links.
    Hooks are stored in the hooks/ directory and symlinked into .git/hooks/
    so that updates to hook files are automatically reflected.
.NOTES
    Admin: Not required (git hooks are in user repository)
#>

function Test-IsSymbolicLink
{
    <#
    .SYNOPSIS
        Test if an item is a symbolic link (cross-edition compatible)
    .DESCRIPTION
        Works in both PowerShell Core and Windows PowerShell (Desktop).
        Core uses .LinkType property, Desktop uses Attributes flag.
    #>
    [OutputType([System.Boolean])]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [System.IO.FileSystemInfo]
        $Item
    )

    if ($null -eq $Item)
    {
        return $false
    }

    # PowerShell Core (6.0+) has LinkType property
    if ($Item.PSObject.Properties.Name -contains 'LinkType')
    {
        return $Item.LinkType -eq 'SymbolicLink'
    }

    # Windows PowerShell (Desktop 5.1) uses Attributes
    return ($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -eq [System.IO.FileAttributes]::ReparsePoint
}

function Install-RepositoryGitHooks
{
    <#
    .SYNOPSIS
        Install git hooks for this repository
    .DESCRIPTION
        Creates symlinks from hooks/ directory to .git/hooks/ directory.
        Makes hook files executable and ensures updates are automatically reflected.
    .PARAMETER root
        Repository root directory
    .PARAMETER DryRun
        When specified, logs actions that would be taken without making modifications
    #>
    # Plural name justified: function installs multiple hooks as batch operation
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root,

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    # Check if this is a git repository
    $gitDir = Join-Path $root ".git"
    if (-not (Test-Path $gitDir))
    {
        Write-Verbose "Skipping git hooks: not a git repository"
        return
    }

    # Check if hooks directory exists
    $hooksSourceDir = Join-Path $root "hooks"
    if (-not (Test-Path $hooksSourceDir))
    {
        Write-Verbose "Skipping git hooks: hooks directory not found"
        return
    }

    Write-Verbose "Reading git hooks from: hooks/"

    $act = $false

    # Get all files in hooks/ directory, excluding non-hook files
    # (config files, documentation, hidden files)
    $excludeExtensions = @('.md', '.txt', '.ini', '.yaml', '.yml', '.json')
    $excludeNames = @('README')

    Write-Verbose "Scanning for hook files (excluding: $($excludeExtensions -join ', ') and hidden files)..."

    $hookFiles = Get-ChildItem -Path $hooksSourceDir -File | Where-Object {
        # Exclude files with non-hook extensions
        $ext = $_.Extension
        if ($excludeExtensions -contains $ext)
        {
            Write-Verbose "Skipping non-hook file: $($_.Name)"
            return $false
        }

        # Exclude specific non-hook filenames
        if ($excludeNames -contains $_.BaseName)
        {
            Write-Verbose "Skipping non-hook file: $($_.Name)"
            return $false
        }

        # Exclude hidden files (starting with .)
        if ($_.Name -like '.*')
        {
            Write-Verbose "Skipping hidden file: $($_.Name)"
            return $false
        }

        return $true
    }

    Write-Verbose "Found $($hookFiles.Count) hook file(s) to install"

    # Ensure .git/hooks directory exists
    $gitHooksDir = Join-Path $gitDir "hooks"
    if (-not (Test-Path $gitHooksDir))
    {
        if ($DryRun)
        {
            Write-Output "DRY-RUN: Would create directory: .git/hooks"
        }
        else
        {
            Write-Verbose "Creating directory: .git/hooks"
            New-Item -ItemType Directory -Path $gitHooksDir -Force | Out-Null
        }
    }

    foreach ($hookFile in $hookFiles)
    {
        $hookName = $hookFile.Name
        $sourcePath = $hookFile.FullName
        $targetPath = Join-Path $gitDir "hooks\$hookName"
        Write-Verbose "Checking hook: $hookName"

        # Check if symlink already exists and points to correct location
        if (Test-Path $targetPath)
        {
            $item = Get-Item $targetPath -Force
            if (Test-IsSymbolicLink -Item $item)
            {
                $linkTarget = $item.Target
                if ($linkTarget -eq $sourcePath)
                {
                    Write-Verbose "Skipping hook $hookName`: already installed"
                    continue
                }
            }
        }

        if (-not $act)
        {
            $act = $true
            Write-Output ":: Installing repository git hooks"
        }

        # Install the hook as a symlink
        if ($DryRun)
        {
            Write-Output "DRY-RUN: Would install hook: $hookName"
        }
        else
        {
            Write-Verbose "Installing hook: $hookName"

            # Remove existing file/symlink if present
            if (Test-Path $targetPath)
            {
                Remove-Item -Path $targetPath -Force
            }

            # Create directory if it doesn't exist
            $targetDir = Split-Path $targetPath -Parent
            if (-not (Test-Path $targetDir))
            {
                New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
            }

            # Create symlink
            New-Item -ItemType SymbolicLink -Path $targetPath -Target $sourcePath -Force | Out-Null
        }
    }
}

Export-ModuleMember -Function Install-RepositoryGitHooks
