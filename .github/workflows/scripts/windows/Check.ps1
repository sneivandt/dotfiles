<#
.SYNOPSIS
    PowerShell shim delegating to .github/workflows/scripts/linux/check.py.

.PARAMETER Stage
    Names of individual stages to run. When omitted, the default stages run.

.PARAMETER All
    Run every stage, including opt-in stages.

.PARAMETER List
    List available stages and exit.

.PARAMETER Strict
    Fail if any requested stage is skipped. Also enabled by DOTFILES_CHECK_STRICT=1.

.EXAMPLE
    pwsh -File .github\workflows\scripts\windows\Check.ps1

.EXAMPLE
    pwsh -File .github\workflows\scripts\windows\Check.ps1 fmt clippy

.EXAMPLE
    pwsh -File .github\workflows\scripts\windows\Check.ps1 -All
#>
[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Stage,

    [switch]$All,

    [switch]$List,

    [switch]$Strict
)

Set-StrictMode -Version Latest

$CheckScript = Join-Path (Split-Path -Parent $PSScriptRoot) 'linux' 'check.py'
$ForwardArgs = @()
if ($Strict) { $ForwardArgs += '--strict' }
if ($List) { $ForwardArgs += '--list' }
elseif ($All) { $ForwardArgs += '--all' }
elseif ($Stage) { $ForwardArgs += $Stage }

if (Get-Command -Name 'python3' -ErrorAction SilentlyContinue)
{
    & python3 -B $CheckScript @ForwardArgs
    exit $LASTEXITCODE
}
if (Get-Command -Name 'python' -ErrorAction SilentlyContinue)
{
    & python -B $CheckScript @ForwardArgs
    exit $LASTEXITCODE
}
if (Get-Command -Name 'py' -ErrorAction SilentlyContinue)
{
    & py -3 -B $CheckScript @ForwardArgs
    exit $LASTEXITCODE
}

Write-Error "Python 3 is required to run $CheckScript"
exit 1
