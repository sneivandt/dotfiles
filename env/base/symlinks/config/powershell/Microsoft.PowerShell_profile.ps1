if (Get-Module PSReadLine)
{
    Set-PSReadLineOption -BellStyle None
}

function Prompt
{
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidUsingWriteHost", "")]
    param (
    )

    $timer = [Diagnostics.Stopwatch]::StartNew()

    $origLastExitCode = $LASTEXITCODE

    $Host.UI.RawUI.ForegroundColor = "White"

    if (($null -eq $env:windir) -and $(Get-Command pwsh).Path -ne $env:SHELL)
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
        $status = git --no-optional-locks status --short --branch --porcelain=v1 --untracked-files=no 2> $null
        $branch = ((($status | Select-Object -First 1) -replace "^## ","") -Split "\.\.\.")[0]

        if ($branch)
        {
            Write-Host " $branch" -NoNewLine -ForegroundColor White

            $changes = ($status -split "\n").Count - 1

            if ($changes -gt 0)
            {
                Write-Host "+$changes" -NoNewLine -ForegroundColor Red
            }
        }
    }

    $elapsedTime = $timer.ElapsedMilliseconds
    Write-Host " $elapsedTime ms" -ForegroundColor Green

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
