# -----------------------------------------------------------------------------
# Helpers.ps1 - Shared test helpers for dotfiles.ps1 wrapper tests
# -----------------------------------------------------------------------------

function Get-RepoRoot {
    return [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
}

function Get-WrapperPath {
    return Join-Path (Get-RepoRoot) 'dotfiles.ps1'
}

function Write-TestStage {
    param([string]$Message)
    Write-Information "=== $Message" -InformationAction Continue
}

function Write-TestPass {
    param([string]$Message)
    Write-Information "PASS: $Message" -InformationAction Continue
}

function Write-TestFail {
    param([string]$Message)
    Write-Information "FAIL: $Message" -InformationAction Continue
}

function Write-NativeStderrShim {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Directory,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )
    $isWindowsPlatform = ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
    $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
    if ($isWindowsPlatform) {
        $shimPath = Join-Path $Directory "$Name.cmd"
        $script = @(
            '@echo off'
            'if defined WRAPPER_TEST_NATIVE_MARKER echo invoked>"%WRAPPER_TEST_NATIVE_MARKER%"'
            '>&2 echo native stderr diagnostic'
            'if defined WRAPPER_TEST_NATIVE_STDOUT echo %WRAPPER_TEST_NATIVE_STDOUT%'
            'exit /b %WRAPPER_TEST_NATIVE_EXIT%'
        ) -join "`r`n"
        [System.IO.File]::WriteAllText($shimPath, "$script`r`n", $utf8NoBom)
        return $shimPath
    }
    $shimPath = Join-Path $Directory $Name
    $script = @(
        '#!/bin/sh'
        'if [ -n "${WRAPPER_TEST_NATIVE_MARKER:-}" ]; then printf "invoked\n" > "$WRAPPER_TEST_NATIVE_MARKER"; fi'
        'printf "native stderr diagnostic\n" >&2'
        'if [ -n "${WRAPPER_TEST_NATIVE_STDOUT:-}" ]; then printf "%s\n" "$WRAPPER_TEST_NATIVE_STDOUT"; fi'
        'exit "${WRAPPER_TEST_NATIVE_EXIT:-0}"'
    ) -join "`n"
    [System.IO.File]::WriteAllText($shimPath, "$script`n", $utf8NoBom)
    & chmod +x $shimPath
    if ($LASTEXITCODE -ne 0) { throw "Failed to mark native shim executable: $shimPath" }
    return $shimPath
}
