#Requires -PSEdition Desktop

function Install-VsCodeExtensions
{
    <#
    .SYNOPSIS
        Install VS Code Extensions
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

    $installed = code --list-extensions

    foreach ($extension in (Get-Content $root\env\base-gui\vscode-extensions.conf))
    {
        if ($installed -notcontains $extension)
        {
            if (-not $act)
            {
                $act = $true

                Write-Output ":: Installing Visual Studio Code Extensions"
            }

            code --install-extension $extension
        }
    }
}
Export-ModuleMember -Function Install-VsCodeExtensions