# -----------------------------------------------------------------------------
# Test-Applications.ps1 - Application-level tests for Windows.
#
# Windows counterpart to scripts/linux/test-applications.sh. Only applications
# that are actually managed on Windows are covered here: zsh, vim, and nvim are
# excluded from the Windows profile (see conf/manifest.toml) and so have no
# Windows equivalent.
#
# Expected: $env:DIR (repo root)
#
# Usage: Test-Applications.ps1 <app> <test> [<test>...]
#        Test-Applications.ps1 git Config Aliases Behavior
# -----------------------------------------------------------------------------

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Application,

    [Parameter(Mandatory = $true, Position = 1, ValueFromRemainingArguments = $true)]
    [string[]]$Tests
)

$ErrorActionPreference = 'Stop'

function Write-TestStage
{
    param([string]$Message)
    Write-Information "=== $Message" -InformationAction Continue
}

function Write-TestPass
{
    param([string]$Message)
    Write-Information "PASS: $Message" -InformationAction Continue
}

function Write-TestFail
{
    param([string]$Message)
    Write-Information "FAIL: $Message" -InformationAction Continue
}

function Test-ToolAvailable
{
    param([Parameter(Mandatory = $true)][string]$Name)
    return $null -ne (Get-Command -Name $Name -ErrorAction SilentlyContinue)
}

# Asserts an effective git config value.
#
# Reading through `git config --get` rather than inspecting files means the
# assertion covers the whole chain: the base symlink, the [include] of
# ~/.config/git/windows, and git's own precedence rules.
function Assert-GitConfig
{
    param(
        [Parameter(Mandatory = $true)][string]$Key,
        [Parameter(Mandatory = $true)][string]$Expected
    )

    $actual = & git config --get $Key 2>$null
    if ($LASTEXITCODE -ne 0)
    {
        $actual = ''
    }
    if ($actual -ne $Expected)
    {
        Write-TestFail "$Key expected '$Expected', got '$actual'"
        throw "Assertion failed: $Key"
    }
    Write-TestPass "$Key = $actual"
}

# ---------------------------------------------------------------------------
# Git
# ---------------------------------------------------------------------------

function Test-GitConfig
{
    if (-not (Test-ToolAvailable 'git'))
    {
        Write-Information 'Skipping: git not installed' -InformationAction Continue
        return
    }
    Write-TestStage 'Testing git configuration'

    $configPath = Join-Path $HOME '.config' 'git' 'config'
    if (-not (Test-Path -LiteralPath $configPath))
    {
        Write-TestFail "custom git config not installed at $configPath"
        throw 'Assertion failed: git config missing'
    }
    Write-TestPass "custom git config found: $configPath"

    Assert-GitConfig -Key 'init.defaultBranch' -Expected 'main'
    Assert-GitConfig -Key 'pull.rebase' -Expected 'true'
    Assert-GitConfig -Key 'merge.conflictstyle' -Expected 'zdiff3'
    Assert-GitConfig -Key 'push.autoSetupRemote' -Expected 'true'
    Assert-GitConfig -Key 'diff.algorithm' -Expected 'histogram'

    # Windows-specific override supplied by symlinks/config/git/windows, which
    # the base config pulls in via [include]. On Linux the same key resolves to
    # 'input', so this asserts the Windows symlink chain end to end.
    Assert-GitConfig -Key 'core.autocrlf' -Expected 'true'
}

function Test-GitAlias
{
    if (-not (Test-ToolAvailable 'git'))
    {
        Write-Information 'Skipping: git not installed' -InformationAction Continue
        return
    }
    Write-TestStage 'Testing git aliases'

    foreach ($alias in @('st', 'br', 'lo', 'ci'))
    {
        $value = & git config --get "alias.$alias" 2>$null
        if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($value))
        {
            Write-TestFail "alias.$alias not defined"
            throw "Assertion failed: alias.$alias"
        }
        Write-TestPass "alias.$alias = $value"
    }
}

function Test-GitBehavior
{
    if (-not (Test-ToolAvailable 'git'))
    {
        Write-Information 'Skipping: git not installed' -InformationAction Continue
        return
    }
    Write-TestStage 'Testing git behavior'

    $repo = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $repo -Force | Out-Null
    $original = Get-Location
    try
    {
        & git init $repo *> $null
        Set-Location -LiteralPath $repo
        & git config user.name 'CI Test' *> $null
        & git config user.email 'ci@test.local' *> $null

        $branch = (& git branch --show-current).Trim()
        if ($branch -ne 'main')
        {
            Write-TestFail "default branch is '$branch', expected 'main'"
            throw 'Assertion failed: default branch'
        }
        Write-TestPass 'default branch is main'

        Set-Content -Path (Join-Path $repo 'test.txt') -Value 'test'
        & git add test.txt *> $null
        & git commit -m 'Test commit' *> $null
        if ($LASTEXITCODE -ne 0)
        {
            Write-TestFail 'commit failed'
            throw 'Assertion failed: commit'
        }
        Write-TestPass 'commit created successfully'
    }
    finally
    {
        Set-Location -LiteralPath $original
        Remove-Item -LiteralPath $repo -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------------------
# Dispatch
# ---------------------------------------------------------------------------

foreach ($test in $Tests)
{
    $function = "Test-$($Application)$($test)"
    if (-not (Get-Command -Name $function -ErrorAction SilentlyContinue))
    {
        throw "Unknown test function: $function"
    }
    & $function
}
