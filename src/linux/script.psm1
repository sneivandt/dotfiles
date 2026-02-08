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
    # Plural name justified: function installs multiple modules as batch operation
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
    $installed_count = 0

    foreach ($module in $modules)
    {
        if (-not (Get-Module -Name $module -ListAvailable -Verbose:$false))
        {
            if (-not $act)
            {
                $act = $true

                Write-Output ":: Installing PowerShell Modules"
            }

            if ($DryRun)
            {
                Write-Output "DRY-RUN: Would install module: $module"
                $installed_count++
            }
            else
            {
                Write-Verbose "Installing module: $module"
                Install-Module -Name $module -AllowClobber -Scope CurrentUser -Force
                $installed_count++
            }
        }
    }

    # Write counter to file for Linux summary (matches logger.sh counter format)
    if ($installed_count -gt 0)
    {
        # Use XDG_CACHE_HOME or ~/.cache for Linux compatibility
        $cache_home = if ($env:XDG_CACHE_HOME) { $env:XDG_CACHE_HOME } else { Join-Path $HOME ".cache" }
        $counter_dir = Join-Path $cache_home "dotfiles/counters"
        if (-not (Test-Path $counter_dir))
        {
            New-Item -Path $counter_dir -ItemType Directory -Force | Out-Null
        }
        $counter_file = Join-Path $counter_dir "powershell_modules_installed"
        $installed_count | Out-File -FilePath $counter_file -Encoding utf8
    }
}
Export-ModuleMember -Function Install-PowerShellModules