# Shared discovery and invocation for the CLI, CI, and staged-file hooks.
[CmdletBinding(PositionalBinding = $false)]
param(
    [string]$Root,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Paths
)

$ErrorActionPreference = 'Stop'

function Find-PowerShellScript
{
    param([string]$Directory)
    if (-not (Get-Item -LiteralPath $Directory -Force).PSIsContainer)
    {
        throw "Not a linter input directory: $Directory"
    }
    foreach ($entry in Get-ChildItem -LiteralPath $Directory -Force)
    {
        if ($entry.PSIsContainer)
        {
            Find-PowerShellScript -Directory $entry.FullName
        }
        elseif ($entry.Extension -in '.ps1', '.psm1', '.psd1')
        {
            $entry.FullName
        }
        else
        {
            $reader = [System.IO.File]::OpenText($entry.FullName)
            try { $firstLine = $reader.ReadLine() }
            finally { $reader.Dispose() }
            if ($firstLine -match '^#!\s*(?:\S*/)?(?:pwsh|powershell)(?:\.exe)?(?:\s|$)' -or
                $firstLine -match '^#!\s*\S*/env(?:\.exe)?\s+(?:-\S+\s+)*(?:pwsh|powershell)(?:\.exe)?(?:\s|$)')
            {
                $entry.FullName
            }
        }
    }
}

if ($Root)
{
    $Paths = @(
        $wrapper = Join-Path $Root 'dotfiles.ps1'
        if (Test-Path -LiteralPath $wrapper) { $wrapper }
        foreach ($dir in 'symlinks', 'hooks', '.github', 'cli/src/app/validation/scripts')
        {
            $path = Join-Path $Root $dir
            if (Test-Path -LiteralPath $path)
            {
                Find-PowerShellScript -Directory $path
            }
        }
    )
}
if (-not $Paths) { exit 0 }
if (-not (Get-Module -ListAvailable -Name PSScriptAnalyzer))
{
    throw 'PSScriptAnalyzer module is not installed'
}
Import-Module PSScriptAnalyzer -Force -ErrorAction Stop
$hasErrors = $false
foreach ($path in $Paths)
{
    $results = Invoke-ScriptAnalyzer -Path $path -Severity Warning,Error -ErrorAction Stop
    if ($results)
    {
        $results | Format-Table -AutoSize
        $hasErrors = $true
    }
}
if ($hasErrors) { exit 1 }
