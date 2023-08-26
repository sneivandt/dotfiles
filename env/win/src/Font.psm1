#Requires -PSEdition Desktop
#Requires -RunAsAdministrator

function Install-Fonts
{
    <#
    .SYNOPSIS
        Install system fonts
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root
    )

    $font = "DejaVu Sans Mono for Powerline"

    if ((Test-Path $env:windir\fonts\$font.ttf) -eq $False -and (Test-Path $env:LOCALAPPDATA\Microsoft\Windows\fonts\$font.ttf) -eq $False)
    {
        Write-Output ":: Installing Fonts"

        $script = Join-Path (Join-Path $root "env\win\fonts") install.ps1

        & $script "$font"
    }
}
Export-ModuleMember -Function Install-Fonts