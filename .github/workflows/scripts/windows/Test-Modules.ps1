#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Tests that all Windows PowerShell modules can be loaded successfully.
.DESCRIPTION
    Loads each PowerShell module in src/windows/ to verify syntax and availability.
#>

param()

$ErrorActionPreference = "Stop"

Write-Output "Testing Windows modules"
$modules = Get-ChildItem .\src\windows\*.psm1

foreach ($module in $modules)
{
    Write-Output "Testing module: $($module.Name)"
    Import-Module $module.FullName -Force
}

Write-Output "All Windows modules loaded successfully"
