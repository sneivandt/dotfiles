<#
.SYNOPSIS
    Canonical local verification entry point (PowerShell twin of .github/workflows/scripts/linux/check.sh).

.DESCRIPTION
    Runs the core repository checks used by CI in one command. Every stage is
    optional-tool aware: if a stage's tool is not installed it is reported as SKIP rather than
    failing, so the script stays usable on a minimal machine.

    Exit status is non-zero if any stage failed.

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

# Individual stages report their own failures; the summary is authoritative.
$ErrorActionPreference = 'Continue'
$Strict = $Strict -or $env:DOTFILES_CHECK_STRICT -eq '1'

$RepoRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)))
$Manifest = Join-Path $RepoRoot 'cli' 'Cargo.toml'
Set-Location (Join-Path $RepoRoot 'cli') -ErrorAction Stop

# Match the profile CI builds with so local failures reproduce CI failures.
$CargoProfile = 'ci'

$DefaultStages = @('fmt', 'clippy', 'test', 'rustdoc', 'config', 'shell', 'powershell', 'audit', 'deny')

# Stages only run when explicitly requested or via -All. 'msrv' downloads a
# second toolchain, which is too slow for the default loop.
$OptionalStages = @('msrv')

function Test-ToolAvailable
{
    param([Parameter(Mandatory = $true)][string]$Name)

    return $null -ne (Get-Command -Name $Name -ErrorAction SilentlyContinue)
}

# Display helpers write to the host rather than the success stream: a stage's
# success stream carries its status and nothing else (see $StageActions).
function Write-Heading
{
    param([Parameter(Mandatory = $true)][string]$Text)

    '' | Out-Host
    "== $Text ==" | Out-Host
}

function Write-Note
{
    param([Parameter(Mandatory = $true)][string]$Text)

    "  $Text" | Out-Host
}

# Runs an external tool and maps its exit code to a stage status.
#
# The tool's own output goes to the host so it stays visible and streaming
# without being mistaken for the status this returns.
function Invoke-Tool
{
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$ToolArguments
    )

    try
    {
        & $FilePath @ToolArguments | Out-Host
    }
    catch
    {
        Write-Note "Cannot run ${FilePath}: $_"
        return 'FAIL'
    }
    if ($LASTEXITCODE -ne 0)
    {
        return 'FAIL'
    }
    return 'pass'
}

function Test-PinnedToolchain
{
    $pin = Select-String -Path (Join-Path $RepoRoot 'cli/rust-toolchain.toml') -Pattern '^channel\s*=\s*"([^"]+)"' |
        Select-Object -First 1
    if (-not $pin)
    {
        Write-Note 'Cannot read pinned Rust version from cli/rust-toolchain.toml'
        return $false
    }
    $expected = $pin.Matches[0].Groups[1].Value
    if (-not (Test-ToolAvailable 'rustc'))
    {
        Write-Note 'rustc not installed'
        return $false
    }
    $actual = & rustc --version
    if ($LASTEXITCODE -ne 0 -or $actual -notlike "rustc $expected *")
    {
        Write-Note "Pinned Rust $expected required; PATH selected $actual."
        Write-Note 'Select the pinned toolchain as described in docs/CONTRIBUTING.md#selecting-the-pinned-toolchain.'
        return $false
    }
    return $true
}

function Invoke-CargoStage
{
    param([Parameter(Mandatory = $true)][string[]]$ToolArguments)

    if (-not (Test-ToolAvailable 'cargo'))
    {
        Write-Note 'cargo not installed; skipping'
        return 'skip'
    }
    if (-not (Test-PinnedToolchain))
    {
        return 'FAIL'
    }
    return Invoke-Tool -FilePath 'cargo' -ToolArguments $ToolArguments
}

# Returns the version of a cargo subcommand, or $null when unavailable.
function Test-CargoSubcommand
{
    param([Parameter(Mandatory = $true)][string]$Name)

    & cargo $Name --version *> $null
    return $LASTEXITCODE -eq 0
}

# Each stage returns exactly one status string: 'pass', 'skip', or 'FAIL'.
#
# The summary switches on that return value, so a stage must not write anything
# else to the success stream - a stray Write-Output or uncaptured tool output
# would make the stage return an array and be reported as a failure. Use
# Write-Note or Out-Host for anything the user should see.
$StageActions = @{
    fmt      = {
        Invoke-CargoStage @('fmt', '--check', '--manifest-path', $Manifest)
    }

    clippy   = {
        Invoke-CargoStage @(
            'clippy', '--locked', '--profile', $CargoProfile, '--manifest-path', $Manifest,
            '--all-targets', '--', '-D', 'warnings'
        )
    }

    test     = {
        Invoke-CargoStage @('test', '--locked', '--profile', $CargoProfile, '--manifest-path', $Manifest)
    }

    rustdoc  = {
        Invoke-CargoStage @(
            'doc', '--locked', '--profile', $CargoProfile, '--manifest-path', $Manifest,
            '--no-deps', '--document-private-items'
        )
    }

    # Runs the CLI's own repository validator against this working tree.
    config   = {
        Invoke-CargoStage @(
            'run', '--locked', '--profile', $CargoProfile, '--manifest-path', $Manifest,
            '--', 'check', '--root', $RepoRoot, '--profile', 'desktop',
            '--only', 'config-diagnostics,symlink-sources,config-files'
        )
    }

    # Delegated to the POSIX twin so the linted file list has a single
    # definition (in cli/src/app/validation/scripts/shellcheck.sh).
    shell    = {
        if (-not (Test-ToolAvailable 'shellcheck'))
        {
            Write-Note 'shellcheck not installed; skipping'
            return 'skip'
        }
        if (-not (Test-ToolAvailable 'sh'))
        {
            Write-Note 'no POSIX sh available; run .github/workflows/scripts/linux/check.sh shell on Linux'
            return 'skip'
        }
        return Invoke-Tool -FilePath 'sh' -ToolArguments @(
            (Join-Path $RepoRoot '.github' 'workflows' 'scripts' 'linux' 'check.sh'), 'shell'
        )
    }

    powershell = {
        if (-not (Get-Module -ListAvailable -Name PSScriptAnalyzer))
        {
            Write-Note 'PSScriptAnalyzer not installed; skipping (Install-Module PSScriptAnalyzer -Scope CurrentUser)'
            return 'skip'
        }
        return Invoke-Tool -FilePath 'pwsh' -ToolArguments @(
            '-NoProfile', '-File',
            (Join-Path $RepoRoot 'cli/src/app/validation/scripts/psscriptanalyzer.ps1'),
            '-Root', $RepoRoot
        )
    }

    audit    = {
        if (-not (Test-ToolAvailable 'cargo'))
        {
            Write-Note 'cargo not installed; skipping'
            return 'skip'
        }
        if (-not (Test-CargoSubcommand 'audit'))
        {
            Write-Note 'cargo-audit not installed; skipping (cargo install cargo-audit)'
            return 'skip'
        }
        return Invoke-Tool -FilePath 'cargo' -ToolArguments @(
            'audit', '--file', (Join-Path $RepoRoot 'cli' 'Cargo.lock')
        )
    }

    deny     = {
        if (-not (Test-ToolAvailable 'cargo'))
        {
            Write-Note 'cargo not installed; skipping'
            return 'skip'
        }
        if (-not (Test-CargoSubcommand 'deny'))
        {
            Write-Note 'cargo-deny not installed; skipping (cargo install cargo-deny)'
            return 'skip'
        }
        return Invoke-Tool -FilePath 'cargo' -ToolArguments @(
            'deny', '--manifest-path', $Manifest, 'check', 'all'
        )
    }

    # Checks the crate still compiles on the declared minimum toolchain.
    msrv     = {
        if (-not (Test-ToolAvailable 'rustup'))
        {
            Write-Note 'rustup not installed; skipping'
            return 'skip'
        }
        $match = Select-String -Path $Manifest -Pattern '^rust-version\s*=\s*"([^"]+)"' |
            Select-Object -First 1
        if (-not $match)
        {
            Write-Note "no rust-version in $Manifest; skipping"
            return 'skip'
        }
        $msrv = $match.Matches[0].Groups[1].Value
        Write-Note "MSRV from Cargo.toml: $msrv"
        & rustup toolchain install $msrv --profile minimal *> $null
        return Invoke-Tool -FilePath 'cargo' -ToolArguments @(
            "+$msrv", 'check', '--locked', '--profile', $CargoProfile, '--manifest-path', $Manifest, '--all-targets'
        )
    }
}

if ($List)
{
    foreach ($name in ($DefaultStages + $OptionalStages))
    {
        Write-Output $name
    }
    exit 0
}

if ($All)
{
    $selected = $DefaultStages + $OptionalStages
}
elseif ($Stage)
{
    $selected = $Stage
}
else
{
    $selected = $DefaultStages
}

# Reject unknown stage names up front so a typo fails loudly instead of
# silently running fewer checks than the caller expects.
$known = $DefaultStages + $OptionalStages
foreach ($name in $selected)
{
    if ($known -notcontains $name)
    {
        Write-Error "Unknown stage: $name. Known stages: $($known -join ', ')"
        exit 2
    }
}

$results = [ordered]@{}
foreach ($name in $selected)
{
    Write-Heading $name
    $results[$name] = & $StageActions[$name]
}

Write-Output ''
Write-Output '== summary =='
$failed = $false
$skipped = $false
foreach ($name in $results.Keys)
{
    $status = $results[$name]
    switch ($status)
    {
        'pass' { Write-Output "  PASS $name" }
        'skip'
        {
            Write-Output "  SKIP $name"
            $skipped = $true
        }
        default
        {
            Write-Output "  FAIL $name"
            $failed = $true
        }
    }
}

if ($failed -or ($Strict -and $skipped))
{
    Write-Error 'Checks failed or required stages were skipped.'
    exit 1
}

Write-Output ''
if ($skipped)
{
    Write-Output 'Some requested checks were skipped; coverage is incomplete.'
}
else
{
    Write-Output 'All checks passed.'
}
exit 0
