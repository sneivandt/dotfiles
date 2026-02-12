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

    # Verify we're in a git repository
    if (-not (Test-Path (Join-Path $Root ".git")))
    {
        Write-VerboseMessage "Skipping Git configuration: not a git repository"
        return
    }

    # Check current symlinks setting
    Push-Location $Root
    try
    {
        Write-VerboseMessage "Checking git configuration: core.symlinks"
        $currentSymlinks = git config --local --get core.symlinks 2>&1 | Out-String
        $currentSymlinks = $currentSymlinks.Trim()
        Write-VerboseMessage "Current core.symlinks value: $(if ($currentSymlinks) { $currentSymlinks } else { '(not set)' })"

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
                Write-VerboseMessage "Setting core.symlinks = false"
                $null = git config --local core.symlinks false 2>&1
            }
        }
        else
        {
            Write-VerboseMessage "Git core.symlinks already configured"
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
        Update dotfiles repository from remote with robust stashing support
    .DESCRIPTION
        Fetches and merges updates from the remote repository, automatically
        stashing any local changes before the update and re-applying them after.
        Provides clear error messages if manual intervention is required.

        The function:
        - Detects all types of working tree changes (staged, unstaged, untracked)
        - Stashes changes before pulling updates
        - Re-applies the stash after successful pull
        - Handles conflicts and errors with clear user guidance
        - Only updates if on the same branch as origin/HEAD

        Idempotent - skips if already up to date.
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
        Write-VerboseMessage "Skipping repository update: not a git repository"
        return
    }

    Write-VerboseMessage "Checking repository status for updates..."

    Push-Location $Root
    try
    {
        # Check if origin/HEAD exists (may not in shallow clones)
        $originHead = git rev-parse --verify --quiet origin/HEAD 2>$null
        if (-not $originHead)
        {
            Write-VerboseMessage "Skipping repository update: origin/HEAD not found (shallow clone or detached HEAD)"
            return
        }

        # Check if current branch matches origin/HEAD
        $originBranch = git rev-parse --abbrev-ref origin/HEAD 2>$null
        $currentBranch = git rev-parse --abbrev-ref HEAD 2>$null

        if ($originBranch)
        {
            $originBranch = $originBranch -replace '^origin/', ''

            if ($currentBranch -ne $originBranch)
            {
                Write-VerboseMessage "Skipping repository update: current branch ($currentBranch) does not match origin/HEAD ($originBranch)"
                return
            }
        }

        # Treat -WhatIf like -DryRun for consistency
        $effectiveDryRun = $DryRun -or $WhatIfPreference

        # Always fetch to ensure we have latest remote refs
        if ($effectiveDryRun)
        {
            Write-Output "DRY-RUN: Would fetch updates from origin"
        }
        elseif ($PSCmdlet.ShouldProcess("dotfiles repository", "Fetch updates from origin"))
        {
            Write-VerboseMessage "Fetching updates from origin"
            git fetch
            if ($LASTEXITCODE -ne 0)
            {
                Write-Warning "Failed to fetch updates from origin. Please check your network connection and try again."
                return
            }
        }

        # Check if local HEAD is behind origin/HEAD
        Write-VerboseMessage "Comparing local HEAD with origin/HEAD..."
        $localHead = git rev-parse HEAD 2>$null
        $remoteHead = git rev-parse origin/HEAD 2>$null

        $localHeadDisplay = if ([string]::IsNullOrEmpty($localHead))
        {
            "<missing>"
        }
        elseif ($localHead.Length -ge 7)
        {
            $localHead.Substring(0, 7)
        }
        else
        {
            $localHead
        }

        $remoteHeadDisplay = if ([string]::IsNullOrEmpty($remoteHead))
        {
            "<missing>"
        }
        elseif ($remoteHead.Length -ge 7)
        {
            $remoteHead.Substring(0, 7)
        }
        else
        {
            $remoteHead
        }

        Write-VerboseMessage "Local HEAD: $localHeadDisplay..."
        Write-VerboseMessage "Remote HEAD: $remoteHeadDisplay..."

        if ($localHead -eq $remoteHead)
        {
            Write-VerboseMessage "Skipping merge: HEAD is up to date with origin/HEAD"
            return
        }

        # We have updates to pull
        Write-Output ":: Updating dotfiles"

        # Check if working tree has changes
        Write-VerboseMessage "Checking if working tree has changes..."
        $status = git status --porcelain 2>$null
        $hasChanges = ($null -ne $status -and $status.Length -gt 0)

        $stashCreated = $false
        $stashName = ""

        if ($hasChanges)
        {
            Write-VerboseMessage "Working tree has changes - will stash before updating"

            if ($effectiveDryRun)
            {
                Write-Output "DRY-RUN: Would stash working tree changes"
            }
            elseif ($PSCmdlet.ShouldProcess("working tree changes", "Stash"))
            {
                # Create stash with timestamp for identification
                $timestamp = Get-Date -Format "yyyy-MM-dd_HH-mm-ss"
                $stashName = "dotfiles-auto-stash-$timestamp"
                Write-VerboseMessage "Creating stash: $stashName"

                # Stash both staged and unstaged changes, including untracked files
                $stashOutput = git stash push -u -m $stashName 2>&1
                if ($LASTEXITCODE -ne 0)
                {
                    Write-Warning "Git stash output: $stashOutput"
                    Write-Warning @"
Failed to stash working tree changes. Please manually stash or commit your
changes before running this script again:
    git stash push -u -m "my-changes"
    .\dotfiles.ps1
    git stash pop
"@
                    return
                }
                $stashCreated = $true
                Write-VerboseMessage "Successfully stashed changes"
            }
        }

        # Perform the merge
        if ($effectiveDryRun)
        {
            Write-Output "DRY-RUN: Would merge updates from origin/HEAD to $currentBranch"
        }
        elseif ($PSCmdlet.ShouldProcess("dotfiles repository", "Merge updates from origin/HEAD to $currentBranch"))
        {
            Write-VerboseMessage "Merging updates from origin/HEAD to $currentBranch"
            $mergeOutput = git merge origin/HEAD 2>&1
            $mergeExitCode = $LASTEXITCODE

            if ($mergeExitCode -ne 0)
            {
                Write-VerboseMessage "Git merge output: $mergeOutput"

                # Merge failed - check if we need to abort
                $inMerge = $null -ne (git rev-parse --verify MERGE_HEAD 2>$null)

                if ($inMerge)
                {
                    Write-Warning "Merge conflict detected. Aborting merge..."
                    git merge --abort 2>&1 | Out-Null
                }

                if ($stashCreated)
                {
                    Write-Warning @"
Failed to merge updates from origin/HEAD due to conflicts.
Your local changes have been preserved in stash: $stashName

To resolve this manually:
    1. Review the conflicts: git diff origin/HEAD
    2. Apply the stash and resolve conflicts:
       git stash apply "stash^{/$stashName}"
    3. Manually merge or resolve conflicts
    4. Once resolved, drop the stash:
       git stash drop "stash^{/$stashName}"

Or discard your local changes and try again:
    git stash drop "stash^{/$stashName}"
    .\dotfiles.ps1
"@
                }
                else
                {
                    Write-Warning @"
Failed to merge updates from origin/HEAD. Your working tree remains unchanged.
Please review and resolve any conflicts manually:
    git status
    git diff origin/HEAD
"@
                }
                return
            }

            Write-VerboseMessage "Successfully merged updates"

            # Re-apply stash if we created one
            if ($stashCreated)
            {
                if ($PSCmdlet.ShouldProcess("stashed changes", "Re-apply"))
                {
                    Write-VerboseMessage "Re-applying stashed changes..."
                    $popOutput = git stash pop 2>&1
                    $popExitCode = $LASTEXITCODE

                    if ($popExitCode -ne 0)
                    {
                        Write-VerboseMessage "Git stash pop output: $popOutput"

                        # Stash pop failed - likely due to conflicts
                        Write-Warning @"
Successfully updated dotfiles, but failed to re-apply your stashed changes
due to conflicts. Your changes are preserved in stash: $stashName

To resolve this manually:
    1. Review the conflicts: git status
    2. Manually apply the stash and resolve conflicts:
       git stash apply "stash^{/$stashName}"
    3. Resolve any conflicts in the affected files
    4. Once resolved, drop the stash:
       git stash drop "stash^{/$stashName}"

Or discard your local changes:
    git stash drop "stash^{/$stashName}"
"@
                        return
                    }

                    Write-VerboseMessage "Successfully re-applied stashed changes"
                    Write-Output "Repository updated successfully and local changes preserved"
                }
            }
            else
            {
                Write-Output "Repository updated successfully"
            }
        }
    }
    finally
    {
        Pop-Location
    }
}

Export-ModuleMember -Function Initialize-GitConfig, Update-DotfilesRepository
