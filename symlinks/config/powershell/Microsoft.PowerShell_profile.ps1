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

if (Get-Command "code-insiders" -ErrorAction SilentlyContinue)
{
    Set-Alias -Name code -Value code-insiders
}

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

# AI / GitHub Copilot CLI aliases
if (Get-Command "gh" -ErrorAction SilentlyContinue)
{
    function Invoke-CopilotChat
    {
        if ($args.Count -eq 0)
        {
            gh copilot -- --yolo
        }
        else
        {
            gh copilot -- --yolo -p ($args -join ' ')
        }
    }

    function Invoke-CopilotSuggest
    {
        gh copilot -i suggest $args
    }

    Set-Alias -Name ai -Value Invoke-CopilotChat
    Set-Alias -Name aic -Value Invoke-CopilotSuggest
}

function Add-PathEntry
{
    param (
        [string]$Directory
    )

    if (-not (Test-Path $Directory))
    {
        return
    }

    $comparison = [System.StringComparison]::Ordinal
    if ($null -ne $env:windir)
    {
        $comparison = [System.StringComparison]::OrdinalIgnoreCase
    }

    $trimChars = [char[]]@(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $normalizedDirectory = $Directory.TrimEnd($trimChars)

    foreach ($entry in @($env:Path -split [System.IO.Path]::PathSeparator))
    {
        if ([string]::IsNullOrWhiteSpace($entry))
        {
            continue
        }

        if ([string]::Equals($entry.TrimEnd($trimChars), $normalizedDirectory, $comparison))
        {
            return
        }
    }

    if ([string]::IsNullOrEmpty($env:Path))
    {
        $env:Path = $Directory
    }
    else
    {
        $env:Path += "$([System.IO.Path]::PathSeparator)$Directory"
    }
}

# Ensure local bin directory is in PATH
$localBinDir = Join-Path (Join-Path $HOME ".local") "bin"
Add-PathEntry -Directory $localBinDir

# Ensure Cargo (Rust) bin directory is in PATH
$cargoDir = Join-Path (Join-Path $HOME ".cargo") "bin"
Add-PathEntry -Directory $cargoDir

# Load additional profile scripts from profile.d directory
$profileDir = Join-Path $HOME ".config\powershell\profile.d"
if (Test-Path $profileDir)
{
    Get-ChildItem -Path $profileDir -Filter "*.ps1" -File |
        Sort-Object -Property FullName |
        ForEach-Object {
            . $_.FullName
        }
}
