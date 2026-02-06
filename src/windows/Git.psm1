<#
.SYNOPSIS
    Git configuration utilities for Windows dotfiles
.DESCRIPTION
    Configures Git settings to ensure smooth operation on Windows, particularly
    around symlink handling which can cause permission issues if not properly
    configured.
.NOTES
    Admin: Not required
#>

function Initialize-GitConfig
{
    <#
    .SYNOPSIS
        Configure Git settings for Windows compatibility
    .DESCRIPTION
        Sets core.symlinks=false to treat repository symlinks as text files
        containing the target path. This prevents permission errors during
        git pull and checkout operations on Windows.

        Idempotent - only sets the value if not already configured.
    .PARAMETER Root
        Root directory of the dotfiles repository
    .PARAMETER DryRun
        If specified, shows what would be done without making changes
    .EXAMPLE
        Initialize-GitConfig -Root $PSScriptRoot -DryRun
        Shows what Git configuration would be applied
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

    # Check current symlinks setting
    Push-Location $Root
    try
    {
        $currentSymlinks = git config --local --get core.symlinks 2>$null

        if ($currentSymlinks -ne 'false')
        {
            if (-not $act)
            {
                $act = $true
                Write-Output ":: Git Configuration"
            }

            if ($DryRun)
            {
                Write-Output "DRY-RUN: Would set git config core.symlinks false"
            }
            else
            {
                Write-Verbose "Setting core.symlinks = false"
                git config --local core.symlinks false
            }
        }
        else
        {
            Write-Verbose "Git core.symlinks already configured"
        }
    }
    finally
    {
        Pop-Location
    }
}

function Update-DotfilesRepository
{
    <#
    .SYNOPSIS
        Update dotfiles repository from remote
    .DESCRIPTION
        Fetches and merges updates from the remote repository when the working
        tree is clean and the local branch is behind the remote. Conservative
        approach: only updates if on the same branch as origin/HEAD and no
        local changes exist.

        Idempotent - skips if already up to date or conditions not met.
    .PARAMETER Root
        Root directory of the dotfiles repository
    .PARAMETER DryRun
        If specified, shows what would be done without making changes
    .EXAMPLE
        Update-DotfilesRepository -Root $PSScriptRoot -DryRun
        Shows what update operations would be performed
    #>
    [CmdletBinding(SupportsShouldProcess = $true)]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Root,

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    # Check if this is a git repository
    if (-not (Test-Path (Join-Path $Root ".git")))
    {
        Write-Verbose "Skipping repository update: not a git repository"
        return
    }

    Push-Location $Root
    try
    {
        # Check if working tree is clean
        $status = git status --porcelain 2>$null
        if ($status)
        {
            Write-Verbose "Skipping repository update: working tree not clean"
            return
        }

        # Check if origin/HEAD exists (may not in shallow clones)
        $originHead = git rev-parse --verify --quiet origin/HEAD 2>$null
        if (-not $originHead)
        {
            Write-Verbose "Skipping repository update: origin/HEAD not found (shallow clone or detached HEAD)"
            return
        }

        # Check if current branch matches origin/HEAD
        $originBranch = git rev-parse --abbrev-ref origin/HEAD 2>$null
        if ($originBranch)
        {
            $originBranch = $originBranch -replace '^origin/', ''
            $currentBranch = git rev-parse --abbrev-ref HEAD 2>$null

            if ($currentBranch -ne $originBranch)
            {
                Write-Verbose "Skipping repository update: current branch ($currentBranch) does not match origin/HEAD ($originBranch)"
                return
            }
        }

        # Always fetch to ensure we have latest remote refs
        if ($DryRun)
        {
            Write-Output "DRY-RUN: Would fetch updates from origin"
        }
        elseif ($PSCmdlet.ShouldProcess("dotfiles repository", "Fetch updates from origin"))
        {
            Write-Verbose "Fetching updates from origin"
            git fetch
        }

        # Check if local HEAD is behind origin/HEAD
        $localHead = git rev-parse HEAD 2>$null
        $remoteHead = git rev-parse origin/HEAD 2>$null

        if ($localHead -ne $remoteHead)
        {
            Write-Output ":: Updating dotfiles"
            if ($DryRun)
            {
                Write-Output "DRY-RUN: Would merge updates from origin/HEAD"
            }
            elseif ($PSCmdlet.ShouldProcess("dotfiles repository", "Merge updates from origin/HEAD"))
            {
                Write-Verbose "Merging updates from origin/HEAD"
                git merge origin/HEAD
            }
        }
        else
        {
            Write-Verbose "Skipping merge: HEAD is up to date with origin/HEAD"
        }
    }
    finally
    {
        Pop-Location
    }
}

Export-ModuleMember -Function Initialize-GitConfig, Update-DotfilesRepository
