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

    # Resolve with git's normal file discovery. Callers run these assertions
    # through Invoke-OutsideRepository, which places git outside any repository
    # and pins GIT_CONFIG_GLOBAL to the installed dotfiles config, so neither
    # repository-local values nor the runner's own ~/.gitconfig can shadow the
    # chain under test.
    $actual = & git config --get $Key 2>$null
    if ($LASTEXITCODE -ne 0)
    {
        $actual = ''
    }
    if ($actual -ne $Expected)
    {
        Write-TestFail "$Key expected '$Expected', got '$actual'"
        # Show every file that contributes this key, in resolution order, so a
        # failure identifies the responsible config file instead of requiring a
        # CI round-trip to diagnose.
        $origins = & git config --show-origin --get-all $Key 2>$null
        if ($origins)
        {
            foreach ($line in $origins)
            {
                Write-Information "    origin: $line" -InformationAction Continue
            }
        }
        else
        {
            Write-Information "    origin: no file defines $Key" -InformationAction Continue
        }
        throw "Assertion failed: $Key"
    }
    Write-TestPass "$Key = $actual"
}

function Invoke-OutsideRepository
{
    <#
    .SYNOPSIS
        Run a script block against the installed dotfiles git configuration,
        from a scratch directory outside any git repository.
    .DESCRIPTION
        Git resolves configuration as local > global > system. Running from the
        CI checkout would let repository-local values shadow the user-level
        configuration these tests validate.

        Git also reads two global files, ~/.config/git/config first and
        ~/.gitconfig second, with the later file winning. The hosted Windows
        runner image ships a ~/.gitconfig that sets core.autocrlf, so it
        shadows the dotfiles chain. GIT_CONFIG_GLOBAL replaces both global
        files with the installed dotfiles config, which still exercises the
        [include] of ~/.config/git/windows and git's precedence rules within
        that chain. An explicit --global scope is not a usable substitute: on
        Windows it does not pick up ~/.config/git/config.
    #>
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Body
    )

    $scratch = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory -Path $scratch -Force | Out-Null
    $configPath = Join-Path $HOME '.config' 'git' 'config'
    $previousGlobal = $env:GIT_CONFIG_GLOBAL
    Push-Location $scratch
    try
    {
        if (Test-Path -LiteralPath $configPath)
        {
            $env:GIT_CONFIG_GLOBAL = $configPath
        }
        & $Body
    }
    finally
    {
        $env:GIT_CONFIG_GLOBAL = $previousGlobal
        Pop-Location
        Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
    }
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

    # Evaluate against the installed dotfiles chain only. The CI checkout
    # carries a repository-local core.autocrlf and the runner image ships a
    # ~/.gitconfig that sets it too; both outrank the user-level chain this
    # test validates.
    Invoke-OutsideRepository {
        Assert-GitConfig -Key 'init.defaultBranch' -Expected 'main'
        Assert-GitConfig -Key 'pull.rebase' -Expected 'true'
        Assert-GitConfig -Key 'merge.conflictstyle' -Expected 'zdiff3'
        Assert-GitConfig -Key 'push.autoSetupRemote' -Expected 'true'
        Assert-GitConfig -Key 'diff.algorithm' -Expected 'histogram'

        # Windows-specific override supplied by symlinks/config/git/windows,
        # which the base config pulls in via [include]. On Linux the same key
        # resolves to 'input', so this asserts the Windows symlink chain end to
        # end.
        $windowsInclude = Join-Path $HOME '.config' 'git' 'windows'
        if (-not (Test-Path -LiteralPath $windowsInclude))
        {
            Write-TestFail "windows git include not installed at $windowsInclude"
            throw 'Assertion failed: git windows include missing'
        }
        Write-TestPass "windows git include found: $windowsInclude"
        Assert-GitConfig -Key 'core.autocrlf' -Expected 'true'
    }
}

function Test-GitAlias
{
    if (-not (Test-ToolAvailable 'git'))
    {
        Write-Information 'Skipping: git not installed' -InformationAction Continue
        return
    }
    Write-TestStage 'Testing git aliases'

    Invoke-OutsideRepository {
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
