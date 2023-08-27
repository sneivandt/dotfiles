#Requires -PSEdition Desktop
#Requires -RunAsAdministrator

<#
.SYNOPSIS
    Dotfiles for Windows
#>

Import-Module $PSScriptRoot\env\win\src\Font.psm1 -Force
Import-Module $PSScriptRoot\env\win\src\Git.psm1 -Force
Import-Module $PSScriptRoot\env\win\src\Registry.psm1 -Force
Import-Module $PSScriptRoot\env\win\src\Symlinks.psm1 -Force
Import-Module $PSScriptRoot\env\win\src\VsCodeExtensions.psm1 -Force

Update-GitSubmodules $PSScriptRoot

Sync-Registry

Install-Fonts $PSScriptRoot
Install-Symlinks $PSScriptRoot
Install-VsCodeExtensions $PSScriptRoot