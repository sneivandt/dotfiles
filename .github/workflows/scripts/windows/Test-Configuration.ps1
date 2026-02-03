<#
.SYNOPSIS
    Validates Windows configuration files are properly structured.
.DESCRIPTION
    Verifies registry.ini and symlinks.ini are parseable and contain expected sections.
#>

param()

$ErrorActionPreference = "Stop"

# Import Profile module (no edition requirement)
Import-Module .\src\windows\Profile.psm1 -Force

# Only import Symlinks module if running Desktop edition (it requires Desktop)
if ($PSVersionTable.PSEdition -eq 'Desktop')
{
    Import-Module .\src\windows\Symlinks.psm1 -Force
}
else
{
    Write-Output "Skipping Symlinks.psm1 import: Requires Desktop edition (running $($PSVersionTable.PSEdition))"
}

# Verify registry.ini is parseable
if (Test-Path .\conf\registry.ini)
{
    Write-Output "✓ registry.ini exists"
    $sections = Get-Content .\conf\registry.ini | Where-Object { $_ -match '^\[.*\]$' }
    Write-Output "✓ Found $($sections.Count) registry sections"
}

# Verify symlinks.ini has Windows section and check specific symlinks
if (Test-Path .\conf\symlinks.ini)
{
    $content = Get-Content .\conf\symlinks.ini -Raw
    if ($content -match '\[windows\]')
    {
        Write-Output "✓ Windows symlinks section found"
    }
    else
    {
        Write-Error "Windows section not found in symlinks.ini"
        exit 1
    }

    # Read and validate Windows-specific symlinks
    $windowsSymlinks = Read-IniSection -FilePath .\conf\symlinks.ini -SectionName "windows"
    Write-Output "Found $($windowsSymlinks.Count) Windows symlinks"

    # Check for expected Windows-specific items
    $expectedItems = @(
        "AppData/Roaming/Code/User/settings.json",
        "config/powershell/Microsoft.PowerShell_profile.ps1",
        "AppData/Local/Packages/Microsoft.WindowsTerminal"
    )

    foreach ($item in $expectedItems)
    {
        $found = $windowsSymlinks | Where-Object { $_ -like "*$item*" }
        if ($found)
        {
            Write-Output "✓ Found expected Windows item: $item"
        }
        else
        {
            Write-Warning "Expected Windows item not found: $item"
        }
    }
}
else
{
    Write-Error "symlinks.ini not found"
    exit 1
}

Write-Output "All Windows configuration assertions passed!"
