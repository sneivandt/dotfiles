<#
.SYNOPSIS
    Tests the Dotfiles PowerShell module manifest and module.
.DESCRIPTION
    Verifies that:
    - The module manifest (Dotfiles.psd1) is valid
    - The module (Dotfiles.psm1) can be imported
    - Exported functions are available
    - The module can be installed to PSModulePath
    - The installed module can be imported by name
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

# Test module import from repository
Write-Output "Testing module import: Dotfiles.psm1 (from repository)"
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

# Test module installation to a temporary PSModulePath
Write-Output "Testing module installation"
# Use cross-platform temp directory
$tempBase = if ($env:TEMP) { $env:TEMP } elseif ($env:TMPDIR) { $env:TMPDIR } else { "/tmp" }
$tempModulesDir = Join-Path $tempBase "dotfiles-test-modules-$(Get-Random)"
try
{
    # Create temporary modules directory and Dotfiles subdirectory
    $installedModulePath = Join-Path $tempModulesDir "Dotfiles"
    New-Item -ItemType Directory -Path $installedModulePath -Force | Out-Null
    Write-Output "  Created temporary module directory: $installedModulePath"

    # Manually copy module files (simulating what Install-DotfilesModule does)
    Copy-Item -Path ".\Dotfiles.psd1" -Destination $installedModulePath -Force
    Copy-Item -Path ".\Dotfiles.psm1" -Destination $installedModulePath -Force
    Write-Output "  ✓ Copied module files"

    # Copy required directories
    $requiredDirs = @("src", "conf", "symlinks")
    foreach ($dir in $requiredDirs)
    {
        $sourcePath = Join-Path $PWD $dir
        $targetPath = Join-Path $installedModulePath $dir
        if (Test-Path $sourcePath)
        {
            Copy-Item -Path $sourcePath -Destination $targetPath -Recurse -Force
            Write-Output "  ✓ Copied directory: $dir"
        }
        else
        {
            throw "Source directory not found: $dir"
        }
    }

    # Verify required directories were copied
    foreach ($dir in $requiredDirs)
    {
        $dirPath = Join-Path $installedModulePath $dir
        if (-not (Test-Path $dirPath))
        {
            throw "Required directory not copied: $dir"
        }
    }

    # Add to PSModulePath temporarily
    $originalModulePath = $env:PSModulePath
    $env:PSModulePath = "$tempModulesDir$([System.IO.Path]::PathSeparator)$env:PSModulePath"
    Write-Output "  Added module directory to PSModulePath"

    # Remove the repository-loaded module
    Remove-Module Dotfiles -Force -ErrorAction SilentlyContinue

    # Try importing the installed module by name
    Write-Output "  Testing import of installed module by name..."
    Import-Module Dotfiles -Force -ErrorAction Stop
    Write-Output "  ✓ Installed module imported successfully"

    # Verify functions are available from installed module
    foreach ($func in $expectedFunctions)
    {
        if (-not (Get-Command $func -ErrorAction SilentlyContinue))
        {
            throw "Function not available from installed module: $func"
        }
        Write-Output "  ✓ $func available from installed module"
    }

    Write-Output "  Success"
}
catch
{
    Write-Output "  Failed: $_"
    exit 1
}
finally
{
    # Restore original PSModulePath
    $env:PSModulePath = $originalModulePath

    # Clean up temporary directory
    if (Test-Path $tempModulesDir)
    {
        Remove-Item -Path $tempModulesDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Output ""
Write-Output "All Dotfiles module tests passed"
