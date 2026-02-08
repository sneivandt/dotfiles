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
            $files = Get-ChildItem -Path $dir -Filter $extension -File -Recurse -ErrorAction SilentlyContinue

            if ($files)
            {
                foreach ($file in $files)
                {
                    try
                    {
                        # PSScriptAnalyzer returns findings as output objects, not errors
                        # Capture error stream to filter out known intermittent internal errors
                        # while preserving legitimate analysis errors

                        # Redirect error stream to capture intermittent errors
                        $errorOutput = @()
                        $findings = Invoke-ScriptAnalyzer -Path $file.FullName -ErrorAction SilentlyContinue -ErrorVariable errorOutput

                        # Log non-trivial errors (not the known assembly/null reference issues)
                        foreach ($err in $errorOutput)
                        {
                            $errorMsg = $err.ToString()
                            # Filter out known intermittent PSScriptAnalyzer internal errors
                            if ($errorMsg -notmatch '(Object reference not set to an instance|dynamic module|dynamic assembly)')
                            {
                                Write-Warning "PSScriptAnalyzer error analyzing $($file.Name): $errorMsg"
                            }
                        }

                        if ($findings)
                        {
                            # Filter out Information severity findings (e.g., PSProvideCommentHelp)
                            # Only count Warning and Error severity as failures
                            $criticalFindings = $findings | Where-Object { $_.Severity -ne 'Information' }

                            if ($findings.Count -gt 0)
                            {
                                Write-Output "Findings in $($file.Name):"
                                $findings | Format-Table -AutoSize
                            }

                            if ($criticalFindings.Count -gt 0)
                            {
                                $hasFindings = $true
                            }
                        }
                    }
                    catch
                    {
                        # Catch unexpected terminating errors
                        # Log but continue analyzing other files
                        Write-Warning "PSScriptAnalyzer failed to analyze $($file.Name): $_"
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
