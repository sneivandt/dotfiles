<#
.SYNOPSIS
    Tests the Dotfiles PowerShell module manifest and module.
.DESCRIPTION
    Verifies that:
    - The module manifest (Dotfiles.psd1) is valid
    - The module (Dotfiles.psm1) can be imported
    - Exported functions are available
#>

param()

$ErrorActionPreference = "Stop"

Write-Output "Testing Dotfiles PowerShell module"
Write-Output "PowerShell Edition: $($PSVersionTable.PSEdition)"
Write-Output ""

# Test module manifest
Write-Output "Testing module manifest: Dotfiles.psd1"
try
{
    $manifest = Test-ModuleManifest -Path .\Dotfiles.psd1 -ErrorAction Stop
    Write-Output "  Module version: $($manifest.Version)"
    Write-Output "  Exported functions: $($manifest.ExportedFunctions.Keys -join ', ')"
    Write-Output "  Success"
}
catch
{
    Write-Output "  Failed: $_"
    exit 1
}

Write-Output ""

# Test module import
Write-Output "Testing module import: Dotfiles.psm1"
try
{
    # First, load the supporting modules that Dotfiles depends on
    $srcModules = Get-ChildItem .\src\windows\*.psm1
    foreach ($module in $srcModules)
    {
        Write-Verbose "Importing supporting module: $($module.Name)"
        Import-Module $module.FullName -Force -ErrorAction Stop
    }

    # Now import the main module
    Import-Module .\Dotfiles.psm1 -Force -ErrorAction Stop
    Write-Output "  Success"
}
catch
{
    Write-Output "  Failed: $_"
    exit 1
}

Write-Output ""

# Verify exported functions
Write-Output "Verifying exported functions"
$expectedFunctions = @('Install-Dotfiles', 'Update-Dotfiles')
$missingFunctions = @()

foreach ($func in $expectedFunctions)
{
    if (Get-Command $func -ErrorAction SilentlyContinue)
    {
        Write-Output "  ✓ $func is available"
    }
    else
    {
        Write-Output "  ✗ $func is NOT available"
        $missingFunctions += $func
    }
}

if ($missingFunctions.Count -gt 0)
{
    Write-Output ""
    Write-Output "Missing functions: $($missingFunctions -join ', ')"
    exit 1
}

Write-Output ""
Write-Output "All Dotfiles module tests passed"
