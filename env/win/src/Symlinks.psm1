#Requires -PSEdition Desktop
#Requires -RunAsAdministrator

function Install-Symlinks
{
    <#
    .SYNOPSIS
        Install symlinks
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root
    )

    $links = Get-Content $root\env\win\symlinks.json | ConvertFrom-Json

    $act = $false

    foreach ($link in $links)
    {
        $p = Join-Path $env:USERPROFILE $link.Target
        $v = Join-Path (Join-Path $root "env") $link.Source

        if (-not (Test-Path $p))
        {
            if (-not $act)
            {
                $act = $true

                Write-Output ":: Installing Symlinks"
            }

            Write-Output "$p -> $v"
            New-Item -Path $p -ItemType SymbolicLink -Value $v -Force > $null
        }
    }
}
Export-ModuleMember -Function Install-Symlinks