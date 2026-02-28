# -----------------------------------------------------------------------------
# Test-InstallUninstall.ps1 — Install/uninstall round-trip test for Windows
# Expected: $env:BINARY_PATH (path to pre-built binary), $env:DIR (repo root)
# -----------------------------------------------------------------------------

$ErrorActionPreference = 'Stop'

function Write-TestStage {
    param([string]$Message)
    Write-Host "═══ $Message" -ForegroundColor Cyan
}

function Write-TestPass {
    param([string]$Message)
    Write-Host "✓ $Message" -ForegroundColor Green
}

function Write-TestFail {
    param([string]$Message)
    Write-Host "✗ $Message" -ForegroundColor Red
}

# Verify that a path exists and is a symlink (reparse point on Windows).
function Assert-Symlink {
    param([string]$Path)
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if (-not $item) {
        Write-TestFail "expected symlink, path does not exist: $Path"
        throw "Assertion failed: symlink missing at $Path"
    }
    if (-not $item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        Write-TestFail "expected symlink (reparse point), but got regular file: $Path"
        throw "Assertion failed: not a symlink at $Path"
    }
    Write-TestPass "symlink exists: $Path"
}

# Verify that a path exists as a regular file or directory (not a symlink).
function Assert-Materialized {
    param([string]$Path)
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if (-not $item) {
        Write-TestFail "expected materialized file/dir after uninstall, path missing: $Path"
        throw "Assertion failed: path missing at $Path"
    }
    if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        Write-TestFail "expected materialized file, still a symlink: $Path"
        throw "Assertion failed: still a symlink at $Path"
    }
    Write-TestPass "materialized: $Path"
}

# ---------------------------------------------------------------------------
# Test the full install → uninstall round-trip for the base profile.
# ---------------------------------------------------------------------------

function Test-InstallUninstallBaseProfile {
    Write-TestStage "Testing install/uninstall round-trip (base profile)"

    if (-not $env:BINARY_PATH) {
        throw "BINARY_PATH environment variable is not set"
    }
    if (-not (Test-Path $env:BINARY_PATH)) {
        throw "Binary not found: $env:BINARY_PATH"
    }
    if (-not $env:DIR) {
        throw "DIR environment variable is not set"
    }

    $homeDir = $env:USERPROFILE
    if (-not $homeDir) { $homeDir = $env:HOME }  # fallback for non-native Windows shells (e.g. Git Bash)

    # Representative symlinks from the [base] section of symlinks.toml
    $gitConfig = Join-Path $homeDir ".config\git\config"
    # Representative symlinks from the [windows] section of symlinks.toml
    $psProfile = Join-Path $homeDir "Documents\PowerShell\Microsoft.PowerShell_profile.ps1"

    Write-Host "Running install..."
    & $env:BINARY_PATH --root $env:DIR -p base install
    if ($LASTEXITCODE -ne 0) {
        throw "Install command failed with exit code $LASTEXITCODE"
    }
    Write-Host "Install complete"

    Assert-Symlink $gitConfig
    Assert-Symlink $psProfile

    Write-Host "Running uninstall..."
    & $env:BINARY_PATH --root $env:DIR -p base uninstall
    if ($LASTEXITCODE -ne 0) {
        throw "Uninstall command failed with exit code $LASTEXITCODE"
    }
    Write-Host "Uninstall complete"

    Assert-Materialized $gitConfig
    Assert-Materialized $psProfile
}

Test-InstallUninstallBaseProfile
Write-Host "`nAll install/uninstall tests passed" -ForegroundColor Green
