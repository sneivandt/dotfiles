if (Get-Module PSReadLine)
{
    Set-PSReadLineOption -BellStyle None
}

function Prompt
{
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidUsingWriteHost", "")]
    param (
    )

    $origLastExitCode = $LASTEXITCODE

    $Host.UI.RawUI.ForegroundColor = "White"

    if (($null -eq $env:windir) -and $(which pwsh) -ne $env:SHELL)
    {
        Write-Host "pwsh " -NoNewLine -ForegroundColor Cyan
    }

    $curPath = $ExecutionContext.SessionState.Path.CurrentLocation.Path

    if ($curPath.StartsWith($Home))
    {
        $curPath = "~" + $curPath.SubString($Home.Length)
    }

    Write-Host $curPath -NoNewLine -ForegroundColor Yellow

    if (Get-Command "git" -ErrorAction SilentlyContinue)
    {
        $gitBranch = $(git rev-parse --abbrev-ref HEAD 2> $null)

        if ($gitBranch)
        {
            Write-Host " $gitBranch" -NoNewLine -ForegroundColor White

            $changes = $(git status --short).Count

            if ($changes -gt 0)
            {
                Write-Host "+$changes" -NoNewLine -ForegroundColor Red
            }
        }
    }

    Write-Host

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
