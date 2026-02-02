#Requires -PSEdition Desktop

<#
.SYNOPSIS
    Windows uninstall script for dotfiles repository.
.DESCRIPTION
    Removes symlinks created by dotfiles.ps1 installation. Does not remove:
      * Git submodules
      * Registry settings (these persist to maintain system configuration)
      * Installed fonts (these persist for system use)
      * VS Code extensions (these persist for user preference)

    This script removes only the symlinks that point to files in the dotfiles
    repository, leaving the user's system in a clean state without dotfiles.
.PARAMETER DryRun
    When specified, logs all actions that would be taken without making
    system modifications. Verbose output is automatically enabled in dry-run
    mode to provide detailed visibility into intended actions.
.NOTES
    This script does not require administrator privileges as it only removes
    symlinks in the user profile.
.EXAMPLE
    PS> .\dotfiles-uninstall.ps1
    Removes all dotfiles-managed symlinks.
.EXAMPLE
    PS> .\dotfiles-uninstall.ps1 -DryRun
    Show what would be removed without making modifications (verbose auto-enabled).
#>

[CmdletBinding()]
param (
    [Parameter(Mandatory = $false)]
    [switch]
    $DryRun
)

# Windows always uses the "windows" profile
$SelectedProfile = "windows"

# Automatically enable verbose output when in dry-run mode
if ($DryRun)
{
    $VerbosePreference = 'Continue'
}

# Import Profile module for INI parsing
Import-Module $PSScriptRoot\src\windows\Profile.psm1 -Force

Write-Output ":: Uninstalling dotfiles (profile: $SelectedProfile)"
if ($DryRun)
{
    Write-Output ":: DRY-RUN MODE: No system modifications will be made"
}

# Get excluded categories for this profile
$excluded = Get-ProfileExclusion -Root $PSScriptRoot -ProfileName $SelectedProfile

$configFile = Join-Path $PSScriptRoot "conf\symlinks.ini"

if (-not (Test-Path $configFile))
{
    Write-Warning "Skipping uninstall: no symlinks.ini found"
    exit 0
}

# Get list of sections from symlinks.ini
$content = Get-Content $configFile
$sections = @()

foreach ($line in $content)
{
    $line = $line.Trim()

    # Extract section headers
    if ($line -match '^\[(.+)\]$')
    {
        $sections += $matches[1]
    }
}

$act = $false
$removedCount = 0
$skippedCount = 0

# Process each section that should be included
foreach ($section in $sections)
{
    # Check if this section/profile should be included
    if (-not (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excluded))
    {
        Write-Verbose "Skipping symlinks section [$section]: profile not included"
        continue
    }

    # Read symlink paths from this section using helper
    $links = Read-IniSection -FilePath $configFile -SectionName $section

    foreach ($link in $links)
    {
        # Target is relative to user profile
        # Convert forward slashes to backslashes for Windows paths
        $targetPath = $link -replace '/', '\'
        # Well-known Windows folders that should NOT be prefixed with a dot
        $wellKnownFolders = @('AppData', 'Documents', 'Downloads', 'Desktop', 'Pictures', 'Music', 'Videos')
        $shouldAddDot = $true

        foreach ($folder in $wellKnownFolders)
        {
            if ($targetPath -like "$folder\\*" -or $targetPath -eq $folder)
            {
                $shouldAddDot = $false
                break
            }
        }

        # Prefix with dot for Unix-style dotfiles (e.g., .config, .ssh)
        if ($shouldAddDot)
        {
            $targetPath = "." + $targetPath
        }
        $targetFullPath = Join-Path $env:USERPROFILE $targetPath

        # Check if symlink exists
        if (Test-Path $targetFullPath)
        {
            $item = Get-Item $targetFullPath -Force -ErrorAction SilentlyContinue
            if ($item -and $item.LinkType -eq 'SymbolicLink')
            {
                # Verify it points to our dotfiles repo before removing
                $currentTarget = $item.Target
                if ($currentTarget -is [array])
                {
                    $currentTarget = $currentTarget[0]
                }

                # Check if the symlink points to this repository
                $resolvedCurrent = [System.IO.Path]::GetFullPath($currentTarget)
                if ($resolvedCurrent -like "$PSScriptRoot\*")
                {
                    if (-not $act)
                    {
                        $act = $true
                        Write-Output ":: Removing Symlinks"
                    }

                    if ($DryRun)
                    {
                        Write-Output "DRY-RUN: Would remove symlink: $targetFullPath"
                        $removedCount++
                    }
                    else
                    {
                        Write-Verbose "Removing symlink: $targetFullPath"
                        try
                        {
                            Remove-Item -Path $targetFullPath -Force -ErrorAction Stop
                            $removedCount++
                        }
                        catch
                        {
                            Write-Warning "Failed to remove symlink $targetFullPath`: $_"
                        }
                    }
                }
                else
                {
                    Write-Verbose "Skipping symlink $link`: points outside dotfiles repository"
                    $skippedCount++
                }
            }
            else
            {
                Write-Verbose "Skipping $link`: not a symlink"
                $skippedCount++
            }
        }
        else
        {
            Write-Verbose "Skipping $link`: does not exist"
            $skippedCount++
        }
    }
}

Write-Output ""
if ($DryRun)
{
    Write-Output "Uninstall complete (dry-run): Would remove $removedCount symlink(s), skip $skippedCount item(s)"
}
else
{
    Write-Output "Uninstall complete: Removed $removedCount symlink(s), skipped $skippedCount item(s)"
}
