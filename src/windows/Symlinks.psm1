#Requires -PSEdition Desktop
#Requires -RunAsAdministrator

function Install-Symlinks
{
    <#
    .SYNOPSIS
        Install symlinks based on profile
    .DESCRIPTION
        Creates symlinks from configuration file, filtering sections based on
        excluded categories. Reads from conf/symlinks.ini sections that match
        the profile (e.g., [windows], [base]).
    .PARAMETER root
        Repository root directory
    .PARAMETER excludedCategories
        Comma-separated list of categories to exclude
    .PARAMETER DryRun
        When specified, logs actions that would be taken without making modifications
    #>
    # Plural name justified: function installs multiple symlinks as batch operation
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root,

        [Parameter(Mandatory = $false)]
        [string]
        $excludedCategories = "",

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    $configFile = Join-Path $root "conf\symlinks.ini"

    if (-not (Test-Path $configFile))
    {
        Write-Verbose "Skipping symlinks: no symlinks.ini found"
        return
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

    # Process each section that should be included
    foreach ($section in $sections)
    {
        # Check if this section/profile should be included
        if (-not (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excludedCategories))
        {
            Write-Verbose "Skipping symlinks section [$section]: profile not included"
            continue
        }

        # Read symlink paths from this section using helper
        $links = Read-IniSection -FilePath $configFile -SectionName $section

        foreach ($link in $links)
        {
            # Check if source file exists (may be excluded by sparse checkout)
            $sourcePath = Join-Path $root "symlinks\$link"

            if (-not (Test-Path $sourcePath))
            {
                Write-Verbose "Skipping symlink $link`: source file excluded"
                continue
            }

            # Target is relative to user profile
            # Convert forward slashes to backslashes for Windows paths
            $targetPath = $link -replace '/', '\'
            # Well-known Windows folders that should NOT be prefixed with a dot
            # Note: If adding new Windows special folders to symlinks.ini, add them here
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
            # Don't prefix Windows-specific paths (e.g., AppData/...)
            if ($shouldAddDot)
            {
                $targetPath = "." + $targetPath
            }
            $targetFullPath = Join-Path $env:USERPROFILE $targetPath

            # Check if symlink exists and points to correct target
            $isCorrectLink = $false
            if (Test-Path $targetFullPath)
            {
                $item = Get-Item $targetFullPath -Force -ErrorAction SilentlyContinue
                if ($item -and $item.LinkType -eq 'SymbolicLink')
                {
                    # Resolve both paths to absolute for comparison
                    $currentTarget = $item.Target
                    if ($currentTarget -is [array])
                    {
                        $currentTarget = $currentTarget[0]
                    }
                    $resolvedCurrent = [System.IO.Path]::GetFullPath($currentTarget)
                    $resolvedSource = [System.IO.Path]::GetFullPath($sourcePath)
                    if ($resolvedCurrent -eq $resolvedSource)
                    {
                        $isCorrectLink = $true
                    }
                }
            }

            if (-not $isCorrectLink)
            {
                if (-not $act)
                {
                    $act = $true
                    Write-Output ":: Installing Symlinks"
                }

                if ($DryRun)
                {
                    if (Test-Path $targetFullPath)
                    {
                        Write-Output "DRY-RUN: Would remove existing: $targetFullPath"
                    }
                    Write-Output "DRY-RUN: Would create symlink: $targetFullPath -> $sourcePath"
                }
                else
                {
                    Write-Verbose "Linking $sourcePath to $targetFullPath"

                    # Ensure parent directory exists
                    $parentDir = Split-Path -Parent $targetFullPath
                    if (-not (Test-Path $parentDir))
                    {
                        New-Item -Path $parentDir -ItemType Directory -Force | Out-Null
                    }

                    # Remove existing file/directory if it exists (to replace with symlink)
                    if (Test-Path $targetFullPath)
                    {
                        Remove-Item -Path $targetFullPath -Recurse -Force
                    }

                    New-Item -Path $targetFullPath -ItemType SymbolicLink -Value $sourcePath -Force > $null
                }
            }
            else
            {
                Write-Verbose "Skipping symlink $link`: already correct"
            }
        }
    }
}
Export-ModuleMember -Function Install-Symlinks