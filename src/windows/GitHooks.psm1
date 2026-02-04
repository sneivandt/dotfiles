<#
.SYNOPSIS
    Git hooks management utilities for the dotfiles repository
.DESCRIPTION
    Installs git hooks for the dotfiles repository as symbolic links.
    Hooks are stored in the hooks/ directory and symlinked into .git/hooks/
    so that updates to hook files are automatically reflected.
#>

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

    $act = $false

    # Get all hook files (exclude README and documentation files)
    $hookFiles = Get-ChildItem -Path $hooksSourceDir -File | Where-Object {
        $_.Name -notmatch '\.(md|txt)$'
    }

    foreach ($hookFile in $hookFiles)
    {
        $hookName = $hookFile.Name
        $sourcePath = $hookFile.FullName
        $targetPath = Join-Path $gitDir "hooks\$hookName"

        # Check if symlink already exists and points to correct location
        if (Test-Path $targetPath)
        {
            $item = Get-Item $targetPath -Force
            if ($item.LinkType -eq "SymbolicLink")
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
