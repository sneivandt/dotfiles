<#
.SYNOPSIS
    Static analysis test module for PowerShell scripts.

.DESCRIPTION
    Provides PSScriptAnalyzer-based static analysis functionality for PowerShell
    scripts in the repository. Scans for .ps1 and .psm1 files and reports any
    violations of PowerShell best practices.
#>

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
        Import-Module -Name "PSScriptAnalyzer" -Force

        $extensions = "*.ps1", "*.psm1"
        $hasFindings = $false

        foreach ($extension in $extensions)
        {
            $files = Get-ChildItem -Path $dir -Filter $extension -Recurse -ErrorAction SilentlyContinue

            if ($files)
            {
                foreach ($file in $files)
                {
                    # Use ErrorAction Continue to handle PSScriptAnalyzer internal errors gracefully
                    # (e.g., "Object reference not set to an instance of an object")
                    $findings = Invoke-ScriptAnalyzer -Path $file.FullName -ErrorAction Continue

                    if ($findings)
                    {
                        Write-Output "Findings in $($file.Name):"
                        $findings | Format-Table -AutoSize
                        $hasFindings = $true
                    }
                }
            }
        }

        if ($hasFindings)
        {
            Write-Error "PSScriptAnalyzer found issues in PowerShell scripts"
        }
    }
    else
    {
        Write-Error "Error: PSScriptAnalyzer not installed"
    }
}
Export-ModuleMember -Function Test-PSScriptAnalyzer
