#Requires -PSEdition Desktop
#Requires -RunAsAdministrator

<#
.SYNOPSIS
    Dotfiles for Windows
#>

foreach ($module in Get-ChildItem $PSScriptRoot\env\win\src\*.psm1)
{
    Import-Module $module.FullName -Force
}

Update-GitSubmodules $PSScriptRoot

Sync-Registry $PSScriptRoot

Install-Fonts $PSScriptRoot
Install-Symlinks $PSScriptRoot
Install-VsCodeExtensions $PSScriptRoot