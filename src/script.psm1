function Install-PowerShellModules
{
    <#
    .SYNOPSIS
            Install PowerShell modules
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    param (
    )

    $modules = @("Az", "PSScriptAnalyzer")

    $act = $false

    foreach ($module in $modules)
    {
        if (-not (Get-Module -Name $module -ListAvailable))
        {
            if (-not $act)
            {
                $act = $true

                Write-Output ":: Installing PowerShell Modules..."
            }

            Install-Module -Name $module -AllowClobber -Scope CurrentUser -Force
        }
    }
}
Export-ModuleMember -Function Install-PowerShellModules

function Test-PSScriptAnalyzer
{
    <#
    .SYNOPSIS
        Analyze PowerShell scripts
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $dir
    )

    if (Get-Module -Name "PSScriptAnalyzer" -ListAvailable)
    {
        Write-Output ":: Running PSScriptAnalyzer..."

        $extensions = "*.ps1", "*.psm1"

        foreach ($extension in $extensions)
        {
            $files = Get-ChildItem -Path $dir -Filter $extension -Recurse

            foreach ($file in $files)
            {
                Invoke-ScriptAnalyzer $file.FullName
            }
        }
    }
    else
    {
        Write-Error "Error: PSScriptAnalyzer not installed"
    }
}
Export-ModuleMember -Function Test-PSScriptAnalyzer