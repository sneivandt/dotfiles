#Requires -PSEdition Core

<#
.SYNOPSIS
    Profile filtering utilities for Windows dotfiles
.DESCRIPTION
    Provides helpers for profile-based section filtering in configuration files,
    similar to the Linux implementation. Reads profile definitions from
    conf/profiles.ini and filters sections accordingly.
.NOTES
    Requires: PowerShell Core
    Admin: Not required
#>

function Test-ShouldIncludeSection
{
    <#
    .SYNOPSIS
        Check if a configuration section should be included based on profile
    .DESCRIPTION
        Filters sections based on comma-separated categories. Returns true if
        ALL required categories are NOT excluded by the current profile.

        Note: This checks section names (comma-separated), not profile names.
        Profile names like "arch-desktop" use hyphens in profiles.ini.
        Section names like [arch,desktop] use commas in other config files.
    .PARAMETER SectionName
        The section name to check (e.g., "windows", "base", "windows,desktop")
    .PARAMETER ExcludedCategories
        Comma-separated list of categories to exclude
    .EXAMPLE
        Test-ShouldIncludeSection -SectionName "windows" -ExcludedCategories "arch,desktop"
        Returns $true (windows not in excluded list)
    .EXAMPLE
        Test-ShouldIncludeSection -SectionName "arch,desktop" -ExcludedCategories "arch"
        Returns $false (arch is in excluded list)
    #>
    [OutputType([System.Boolean])]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $SectionName,

        [Parameter(Mandatory = $false)]
        [string]
        $ExcludedCategories = ""
    )

    # Empty section name means always include
    if ([string]::IsNullOrWhiteSpace($SectionName))
    {
        return $true
    }

    # If no categories are excluded, include everything
    if ([string]::IsNullOrWhiteSpace($ExcludedCategories))
    {
        return $true
    }

    # Split section into required categories
    $requiredCategories = $SectionName -split ',' | ForEach-Object { $_.Trim() }

    # Split excluded categories
    $excludedList = $ExcludedCategories -split ',' | ForEach-Object { $_.Trim() }

    # Check if ANY required category is excluded
    foreach ($category in $requiredCategories)
    {
        if ($excludedList -contains $category)
        {
            # This category is excluded, so exclude the section
            return $false
        }
    }

    # All required categories are available, include the section
    return $true
}

function Get-ProfileExclusion
{
    <#
    .SYNOPSIS
        Get excluded categories for a profile
    .DESCRIPTION
        Reads conf/profiles.ini and returns the comma-separated list of
        categories to exclude for the specified profile.
    .PARAMETER Root
        Repository root directory
    .PARAMETER ProfileName
        Profile name (e.g., "windows", "arch-desktop")
    .EXAMPLE
        Get-ProfileExclusion -Root $PSScriptRoot -ProfileName "windows"
        Returns "arch,desktop" (as defined in profiles.ini)
    #>
    [OutputType([System.String])]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Root,

        [Parameter(Mandatory = $true)]
        [string]
        $ProfileName
    )

    $profilesFile = Join-Path $Root "conf\profiles.ini"

    if (-not (Test-Path $profilesFile))
    {
        Write-Warning "profiles.ini not found at $profilesFile"
        return ""
    }

    $content = Get-Content $profilesFile
    $inProfile = $false
    $exclude = ""

    foreach ($line in $content)
    {
        $line = $line.Trim()

        # Skip empty lines and comments
        if ($line.Length -eq 0 -or $line -match '^\s*#')
        {
            continue
        }

        # Check for section header
        if ($line -match '^\[(.+)\]$')
        {
            $inProfile = ($matches[1] -eq $ProfileName)
            continue
        }

        # Parse key=value within target profile
        if ($inProfile -and $line -match '^(.+?)\s*=\s*(.*)$')
        {
            $key = $matches[1].Trim()
            $value = $matches[2].Trim()

            if ($key -eq 'exclude')
            {
                $exclude = $value
            }
        }
    }

    # Build complete exclusion list
    $allExcludes = @()

    if ($exclude)
    {
        $allExcludes += $exclude -split ',' | ForEach-Object { $_.Trim() }
    }

    # Always exclude Arch Linux specific category on Windows (safety net in case
    # profiles.ini is misconfigured - Windows profiles should always exclude 'arch')
    if ($allExcludes -notcontains 'arch')
    {
        $allExcludes += 'arch'
    }

    return ($allExcludes -join ',')
}

function Read-IniSection
{
    <#
    .SYNOPSIS
        Read entries from an INI file section
    .DESCRIPTION
        Generic INI file section reader. Returns all non-empty, non-comment lines
        within the specified section.
    .PARAMETER FilePath
        Path to INI file
    .PARAMETER SectionName
        Section name to read
    .EXAMPLE
        Read-IniSection -FilePath "conf\packages.ini" -SectionName "windows"
        Returns array of package names from [windows] section
    #>
    [OutputType([System.Object[]])]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $FilePath,

        [Parameter(Mandatory = $true)]
        [string]
        $SectionName
    )

    if (-not (Test-Path $FilePath))
    {
        return @()
    }

    $content = Get-Content $FilePath
    $inSection = $false
    $entries = @()

    foreach ($line in $content)
    {
        $line = $line.Trim()

        # Skip empty lines and comments
        if ($line.Length -eq 0 -or $line -match '^\s*#')
        {
            continue
        }

        # Check for section header
        if ($line -match '^\[(.+)\]$')
        {
            # If we were in target section, we're done
            if ($inSection)
            {
                break
            }

            # Check if this is our target section
            $inSection = ($matches[1] -eq $SectionName)
            continue
        }

        # Add lines within target section
        if ($inSection)
        {
            $entries += $line
        }
    }

    return $entries
}

Export-ModuleMember -Function Test-ShouldIncludeSection, Get-ProfileExclusion, Read-IniSection
