<#
.SYNOPSIS
    Symlink management utilities for Windows dotfiles
.DESCRIPTION
    Creates and manages symbolic links from the symlinks/ directory to their
    target locations in the user's profile. Supports profile-based filtering
    to only install symlinks relevant to the active profile.
.NOTES
    Admin: Required for creating symbolic links (not required in dry-run mode)
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

            # Check if source is a path reference file (contains only a relative path)
            # This allows sharing configs between Linux (.config) and Windows (AppData) locations
            if (Test-Path $sourcePath -PathType Leaf)
            {
                $content = Get-Content $sourcePath -Raw -ErrorAction SilentlyContinue
                if ($content -and $content.Trim() -match '^(\.\./)+[^\s]+$')
                {
                    # File contains only a relative path - resolve to actual config file
                    $sourceDir = Split-Path -Parent $sourcePath
                    $referencedPath = Join-Path $sourceDir $content.Trim()
                    $resolvedPath = [System.IO.Path]::GetFullPath($referencedPath)

                    if (Test-Path $resolvedPath)
                    {
                        Write-Verbose "Resolved $link reference to $(Split-Path -Leaf $resolvedPath)"
                        $sourcePath = $resolvedPath
                    }
                    else
                    {
                        Write-Verbose "Skipping symlink $link`: referenced file not found at $resolvedPath"
                        continue
                    }
                }
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
                if ($targetPath.StartsWith("$folder\", [StringComparison]::OrdinalIgnoreCase) -or $targetPath -eq $folder)
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
                if (Test-IsSymbolicLink -Item $item)
                {
                    # Resolve both paths to absolute for comparison
                    $currentTarget = $item.Target
                    if ($currentTarget -is [array])
                    {
                        $currentTarget = $currentTarget[0]
                    }
                    # Handle relative paths by resolving from the symlink's directory
                    if (-not [System.IO.Path]::IsPathRooted($currentTarget))
                    {
                        $linkDir = Split-Path -Parent $targetFullPath
                        $resolvedCurrent = [System.IO.Path]::GetFullPath((Join-Path $linkDir $currentTarget))
                    }
                    else
                    {
                        $resolvedCurrent = [System.IO.Path]::GetFullPath($currentTarget)
                    }
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
                        try
                        {
                            New-Item -Path $parentDir -ItemType Directory -Force | Out-Null
                        }
                        catch
                        {
                            Write-Warning "Failed to create parent directory $parentDir`: $_"
                            continue
                        }
                    }

                    # Remove existing file/directory if it exists (to replace with symlink)
                    if (Test-Path $targetFullPath)
                    {
                        try
                        {
                            Remove-Item -Path $targetFullPath -Recurse -Force
                        }
                        catch
                        {
                            Write-Warning "Failed to remove existing item at $targetFullPath`: $_"
                            continue
                        }
                    }

                    try
                    {
                        New-Item -Path $targetFullPath -ItemType SymbolicLink -Value $sourcePath -Force -ErrorAction Stop | Out-Null
                    }
                    catch
                    {
                        Write-Warning "Failed to create symlink at $targetFullPath`: $_"
                    }
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
