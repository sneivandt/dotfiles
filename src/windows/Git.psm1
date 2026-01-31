#Requires -PSEdition Desktop

function Update-GitSubmodules
{
    <#
    .SYNOPSIS
        Update git submodules
    .DESCRIPTION
        Reads submodules from conf/submodules.ini. Initializes and updates
        submodules that are uninitialized or out of date.
    .PARAMETER root
        Repository root directory
    .PARAMETER DryRun
        When specified, logs actions that would be taken without making modifications
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseShouldProcessForStateChangingFunctions", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root,

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    $configFile = Join-Path $root "conf\submodules.ini"

    if (-not (Test-Path $configFile))
    {
        Write-Verbose "Skipping git submodules: no submodules.ini found"
        return
    }

    # Read submodules from all sections in submodules.ini
    # Note: Submodules are intentionally not filtered by profile - all submodules
    # are initialized regardless of active profile to maintain repository integrity.
    $content = Get-Content $configFile
    $modules = @()
    $inSection = $false

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
            $inSection = $true
            continue
        }

        # Add module entries
        if ($inSection)
        {
            $modules += $line
        }
    }

    if ($modules.Count -eq 0)
    {
        Write-Verbose "Skipping git submodules: no modules configured"
        return
    }

    # Remove duplicates
    $modules = $modules | Select-Object -Unique

    Push-Location $root

    # Check if any submodules need updating (uninitialized or modified)
    $status = git submodule status $modules

    if ($status -match "^[\+\-]")
    {
        Write-Output ":: Installing Git Submodules"

        if ($DryRun)
        {
            Write-Output "DRY-RUN: Would update submodules: $($modules -join ' ')"
        }
        else
        {
            Write-Verbose "Updating submodules: $($modules -join ' ')"
            git submodule update --init --recursive $modules
        }
    }
    else
    {
        Write-Verbose "Skipping git submodules: already up to date"
    }

    Pop-Location
}
Export-ModuleMember -Function Update-GitSubmodules