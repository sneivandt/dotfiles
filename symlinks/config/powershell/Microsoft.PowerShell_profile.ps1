# Global variables justified: profile file maintains session state
[Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidGlobalVars", "")]
param()

if (Get-Module PSReadLine)
{
    Set-PSReadLineOption -BellStyle None
    Set-PSReadLineOption -HistorySearchCursorMovesToEnd
    Set-PSReadLineOption -MaximumHistoryCount 10000
}

$Global:GitExists = [bool](Get-Command "git" -ErrorAction SilentlyContinue)

$Global:IsNestedPwsh = $false
if ($null -eq $env:windir)
{
    $pwshCmd = Get-Command "pwsh" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -ne $pwshCmd -and $pwshCmd.Path -ne $env:SHELL)
    {
        $Global:IsNestedPwsh = $true
    }
}

function Prompt
{
    # Write-Host justified: direct console output for prompt display (not pipeline data)
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidUsingWriteHost", "")]
    param (
    )

    $origLastExitCode = $LASTEXITCODE

    $Host.UI.RawUI.ForegroundColor = "White"

    if ($Global:IsNestedPwsh)
    {
        Write-Host "pwsh " -NoNewLine -ForegroundColor Cyan
    }

    $curPath = $ExecutionContext.SessionState.Path.CurrentLocation.Path

    if ($curPath.StartsWith($Home, [System.StringComparison]::OrdinalIgnoreCase))
    {
        $curPath = "~" + $curPath.SubString($Home.Length)
    }

    Write-Host $curPath -NoNewLine -ForegroundColor Yellow

    if ($Global:GitExists)
    {
        # Performance: Use --porcelain=v1 and --untracked-files=no for faster git status
        $status = @(git --no-optional-locks status --short --branch --porcelain=v1 --untracked-files=no 2> $null)

        if ($status.Count -gt 0)
        {
            $branchLine = $status[0]
            if ($branchLine.StartsWith("## "))
            {
                $branchName = $branchLine.Substring(3)
                $ellipsis = $branchName.IndexOf("...")
                if ($ellipsis -gt 0) { $branchName = $branchName.Substring(0, $ellipsis) }

                Write-Host " $branchName" -NoNewLine -ForegroundColor White

                $changes = $status.Count - 1
                if ($changes -gt 0)
                {
                    Write-Host "+$changes" -NoNewLine -ForegroundColor Red
                }
            }
        }
    }

    Write-Host ""

    if ($env:username -eq "root")
    {
        "# "
    }
    else
    {
        "$ "
    }

    $LASTEXITCODE = $origLastExitCode
}
