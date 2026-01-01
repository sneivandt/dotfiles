function Install-PowerShellModules
{
    <#
    .SYNOPSIS
        Ensure required user-scoped PowerShell modules are installed.
    .DESCRIPTION
        Installs a curated set of modules needed by tooling / validation.
        Safe to re-run; existing modules are skipped. Uses CurrentUser scope
        to avoid requiring elevation after initial repository bootstrap.
    .NOTES
        Extend the $modules array cautiously—large meta modules (e.g. Az)
        increase initial provisioning time.
    .EXAMPLE
        PS> Install-PowerShellModules
        Installs Az & PSScriptAnalyzer if missing.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    param (
    )

    # Modules to guarantee. Order is not significant currently.
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
        Run PSScriptAnalyzer across repository PowerShell sources.
    .DESCRIPTION
        Recursively scans for .ps1 / .psm1 files beneath the provided path
        and executes Invoke-ScriptAnalyzer on each. Emits findings to output
        (non-terminating). If the analyzer module is absent, writes an error
        so CI can flag missing dependency.
    .PARAMETER dir
        Root directory to traverse for PowerShell scripts.
    .EXAMPLE
        PS> Test-PSScriptAnalyzer -dir $PSScriptRoot
        Lints all PowerShell scripts under the repository.
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