function Install-PowerShellModules
{
    <#
    .SYNOPSIS
        Ensure required user-scoped PowerShell modules are installed.
    .DESCRIPTION
        Installs a curated set of modules needed by tooling / validation.
        Safe to re-run; existing modules are skipped. Uses CurrentUser scope
        to avoid requiring elevation after initial repository bootstrap.
    .PARAMETER DryRun
        When specified, logs actions that would be taken without making modifications.
    .NOTES
        Extend the $modules array cautiously—large meta modules (e.g. Az)
        increase initial provisioning time.
    .EXAMPLE
        PS> Install-PowerShellModules
        Installs Az & PSScriptAnalyzer if missing.
    .EXAMPLE
        PS> Install-PowerShellModules -DryRun
        Shows what modules would be installed without actually installing them.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    # Modules to guarantee. Order is not significant currently.
    $modules = @("Az", "PSScriptAnalyzer")

    $act = $false

    foreach ($module in $modules)
    {
        if (-not (Get-Module -Name $module -ListAvailable -Verbose:$false))
        {
            if (-not $act)
            {
                $act = $true

                Write-Output ":: Installing PowerShell Modules..."
            }

            if ($DryRun)
            {
                Write-Output "DRY-RUN: Would install module: $module"
            }
            else
            {
                Write-Verbose "Installing module: $module"
                Install-Module -Name $module -AllowClobber -Scope CurrentUser -Force
            }
        }
    }
}
Export-ModuleMember -Function Install-PowerShellModules