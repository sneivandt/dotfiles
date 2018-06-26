if (Get-Module PSReadLine)
{
    Set-PSReadLineOption -BellStyle None
}

function prompt
{
    $origLastExitCode = $LASTEXITCODE

    $isAdmin = $env:username -eq "root"

    $curPath = $ExecutionContext.SessionState.Path.CurrentLocation.Path
    if ($curPath.ToLower().StartsWith($Home.ToLower()))
    {
        $curPath = "~" + $curPath.SubString($Home.Length)
    }

    $ps1 = $curPath

    $gitBranch = $(git rev-parse --abbrev-ref HEAD 2> $null)
    
    if ($gitBranch)
    {
        $ps1 += " "
        $ps1 += $gitBranch

        $changes = $($(git status --short) | Measure-Object -Line).Lines
        if ($changes -gt 0) 
        {
            $ps1 += "+"
            $ps1 += $changes
        }
    }

    if ($isAdmin)
    {
        $ps1 += "`n# "
    }
    else
    {
        $ps1 += "`n$ "
    }

    $ps1

    $LASTEXITCODE = $origLastExitCode
}