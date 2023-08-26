#Requires -PSEdition Desktop

function Update-GitSubmodules
{
    <#
    .SYNOPSIS
        Update git submodules
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseShouldProcessForStateChangingFunctions", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root
    )

    $act = $false

    Push-Location $root
    foreach ($module in ("env/base-gui", "env/win/fonts"))
    {
        $location = (Join-Path $root $module)
        $status = git submodule status -- $location

        if ($status.Substring(0, 1) -match "\+|\-")
        {
            if (-not $act)
            {
                $act = $true

                Write-Output ":: Updating Git Submodules"
            }

            git submodule update --init --recursive -- $location
        }
    }
    Pop-Location
}
Export-ModuleMember -Function Update-GitSubmodules