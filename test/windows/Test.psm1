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

        foreach ($extension in $extensions)
        {
            $files = Get-ChildItem -Path $dir -Filter $extension -Recurse -ErrorAction SilentlyContinue

            if ($files)
            {
                foreach ($file in $files)
                {
                    Invoke-ScriptAnalyzer -Path $file.FullName
                }
            }
        }
    }
    else
    {
        Write-Error "Error: PSScriptAnalyzer not installed"
    }
}
Export-ModuleMember -Function Test-PSScriptAnalyzer
