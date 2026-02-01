#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Tests that all Windows PowerShell modules can be loaded successfully.
.DESCRIPTION
    Loads each PowerShell module in src/windows/ to verify syntax and availability.
    Modules requiring Desktop edition are skipped when running in Core edition.
#>

param()

$ErrorActionPreference = "Stop"

Write-Output "Testing Windows modules"
Write-Output "PowerShell Edition: $($PSVersionTable.PSEdition)"
Write-Output ""

$modules = Get-ChildItem .\src\windows\*.psm1
$failedModules = @()
$skippedModules = @()

foreach ($module in $modules)
{
    Write-Output "Testing module: $($module.Name)"

    # Check if module requires Desktop edition
    $content = Get-Content $module.FullName -Raw
    $requiresDesktop = $content -match '#Requires\s+-PSEdition\s+Desktop'

    if ($requiresDesktop -and $PSVersionTable.PSEdition -eq 'Core')
    {
        Write-Output "  Skipped: Requires Desktop edition (running in Core)"
        $skippedModules += $module.Name
        continue
    }

    try
    {
        Import-Module $module.FullName -Force -ErrorAction Stop
        Write-Output "  Success"
    }
    catch
    {
        Write-Output "  Failed: $_"
        $failedModules += $module.Name
    }
}

Write-Output ""
Write-Output "Summary:"
Write-Output "  Total modules: $($modules.Count)"
Write-Output "  Skipped (Desktop-only): $($skippedModules.Count)"
Write-Output "  Failed: $($failedModules.Count)"

if ($failedModules.Count -gt 0)
{
    Write-Output ""
    Write-Output "Failed modules:"
    $failedModules | ForEach-Object { Write-Output "  - $_" }
    exit 1
}

Write-Output ""
Write-Output "All compatible Windows modules loaded successfully"
