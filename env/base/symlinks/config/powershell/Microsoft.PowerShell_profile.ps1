if (Get-Module PSReadLine)
{
    Set-PSReadLineOption -BellStyle None
}

function prompt
{
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidUsingWriteHost", "")]
    param (
    )

    $origLastExitCode = $LASTEXITCODE

    if ($(which pwsh) -ne $env:SHELL)
    {
        Write-Host "pwsh " -NoNewLine -ForegroundColor Cyan
    }

    $curPath = $ExecutionContext.SessionState.Path.CurrentLocation.Path
    if ($curPath.StartsWith($Home))
    {
        $curPath = "~" + $curPath.SubString($Home.Length)
    }
    Write-Host $curPath -NoNewLine -ForegroundColor Yellow

    $gitBranch = $(git rev-parse --abbrev-ref HEAD 2> $null)
    if ($gitBranch)
    {
        Write-Host " $gitBranch" -NoNewLine -ForegroundColor White
        $changes = $($(git status --short) | Measure-Object -Line).Lines
        if ($changes -gt 0)
        {
            Write-Host "+$changes" -NoNewLine -ForegroundColor Red
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
