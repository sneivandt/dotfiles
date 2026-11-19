# -----------------------------------------------------------------------------
# Test-InstallUninstall.ps1 - Install/uninstall round-trip test for Windows
# Expected: $env:BINARY_PATH (path to pre-built binary), $env:DIR (repo root)
# -----------------------------------------------------------------------------

$ErrorActionPreference = 'Stop'

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

# Verify that a path exists and is a symlink to the expected source.
function Assert-Symlink {
    param(
        [string]$Path,
        [string]$ExpectedTarget
    )
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if (-not $item) {
        Write-TestFail "expected symlink, path does not exist: $Path"
        throw "Assertion failed: symlink missing at $Path"
    }
    if (-not $item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        Write-TestFail "expected symlink (reparse point), but got regular file: $Path"
        throw "Assertion failed: not a symlink at $Path"
    }

    $rawTarget = @($item.Target)[0]
    if (-not $rawTarget) {
        Write-TestFail "symlink has no target: $Path"
        throw "Assertion failed: symlink target missing at $Path"
    }
    if (-not [System.IO.Path]::IsPathRooted($rawTarget)) {
        $rawTarget = Join-Path $item.DirectoryName $rawTarget
    }

    $actualTarget = [System.IO.Path]::GetFullPath($rawTarget).TrimEnd('\')
    $expected = (Get-Item -LiteralPath $ExpectedTarget -Force).FullName.TrimEnd('\')
    if (-not [string]::Equals($actualTarget, $expected, [System.StringComparison]::OrdinalIgnoreCase)) {
        Write-TestFail "symlink $Path points to '$actualTarget', expected '$expected'"
        throw "Assertion failed: wrong symlink target at $Path"
    }
    Write-TestPass "symlink target: $Path -> $expected"
}

# Verify that a path is materialized and preserves the installed source content.
function Assert-Materialized {
    param(
        [string]$Path,
        [string]$ExpectedSource,
        [string]$InstalledSnapshot
    )
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if (-not $item) {
        Write-TestFail "expected materialized file/dir after uninstall, path missing: $Path"
        throw "Assertion failed: path missing at $Path"
    }
    if ($item.Attributes.HasFlag([System.IO.FileAttributes]::ReparsePoint)) {
        Write-TestFail "expected materialized file, still a symlink: $Path"
        throw "Assertion failed: still a symlink at $Path"
    }

    $actualHash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    $sourceHash = (Get-FileHash -LiteralPath $ExpectedSource -Algorithm SHA256).Hash
    $snapshotHash = (Get-FileHash -LiteralPath $InstalledSnapshot -Algorithm SHA256).Hash
    if ($actualHash -ne $sourceHash -or $actualHash -ne $snapshotHash) {
        Write-TestFail "materialized content does not match the installed source: $Path"
        throw "Assertion failed: materialized content changed at $Path"
    }
    Write-TestPass "materialized content: $Path"
}

# Capture the link's timestamps rather than its target's timestamps.
function Get-LinkState {
    param([string]$Path)
    $item = Get-Item -LiteralPath $Path -Force
    return "$($item.CreationTimeUtc.Ticks):$($item.LastWriteTimeUtc.Ticks)"
}

function Assert-InstalledUnchanged {
    param(
        [string]$Path,
        [string]$ExpectedSource,
        [string]$InstalledSnapshot,
        [string]$LinkState
    )
    Assert-Symlink $Path $ExpectedSource
    $actualHash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    $snapshotHash = (Get-FileHash -LiteralPath $InstalledSnapshot -Algorithm SHA256).Hash
    if ((Get-LinkState $Path) -ne $LinkState -or $actualHash -ne $snapshotHash) {
        throw "Assertion failed: second apply changed installed link or content: $Path"
    }
    Write-TestPass "second apply preserved link and content: $Path"
}

function Assert-ProfileStartup {
    param([string]$Shell, [string]$Path, [string]$ExpectedLauncher, [string]$Version)

    $env:DOTFILES_TEST_PROFILE = $Path
    $env:DOTFILES_TEST_LAUNCHER = $ExpectedLauncher
    try {
        $command = @'
            $ErrorActionPreference = 'Stop'
            Set-Variable -Name HOME -Value $env:USERPROFILE -Force
            . $env:DOTFILES_TEST_PROFILE
            if ((Get-Command dotfiles.cmd).Source -ne $env:DOTFILES_TEST_LAUNCHER) {
                throw 'The installed profile did not add the fixture launcher to PATH'
            }
            dot --version
            exit $LASTEXITCODE
'@
        $output = @(& $Shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command $command)
        if ($LASTEXITCODE -ne 0 -or $output.Count -ne 1 -or $output[0] -cne $Version) {
            throw "$Shell did not start the fixture CLI through the installed profile"
        }
    } finally {
        Remove-Item Env:DOTFILES_TEST_PROFILE, Env:DOTFILES_TEST_LAUNCHER
    }
}

# ---------------------------------------------------------------------------
# Test the full install -> uninstall round-trip for the base profile.
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
    $gitConfigSource = Join-Path $env:DIR "symlinks\config\git\config"
    # Representative symlinks from the [windows] section of symlinks.toml
    $psProfile = Join-Path $homeDir "Documents\PowerShell\Microsoft.PowerShell_profile.ps1"
    $psProfileSource = Join-Path $env:DIR "symlinks\config\powershell\Microsoft.PowerShell_profile.ps1"
    $legacyProfile = Join-Path $homeDir "Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1"
    $terminalRelative = 'AppData\Local\Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json'
    $terminal = Join-Path $homeDir $terminalRelative
    $terminalSource = Join-Path (Join-Path $env:DIR 'symlinks') $terminalRelative
    $launcher = Join-Path $homeDir '.local\bin\dotfiles.cmd'

    $gitConfigSnapshot = [System.IO.Path]::GetTempFileName()
    $psProfileSnapshot = [System.IO.Path]::GetTempFileName()
    $terminalSnapshot = [System.IO.Path]::GetTempFileName()
    try {
        Write-Information "Running install..." -InformationAction Continue
        & $env:BINARY_PATH update --no-repo-update --root $env:DIR -p base --skip packages,agent-packages,vscode-extensions
        if ($LASTEXITCODE -ne 0) {
            throw "Install command failed with exit code $LASTEXITCODE"
        }
        Write-Information "Install complete" -InformationAction Continue

        Assert-Symlink $gitConfig $gitConfigSource
        Assert-Symlink $psProfile $psProfileSource
        Assert-Symlink $legacyProfile $psProfileSource
        Assert-Symlink $terminal $terminalSource
        Copy-Item -LiteralPath $terminal -Destination $terminalSnapshot -Force
        $version = & $env:BINARY_PATH --version
        if ($LASTEXITCODE -ne 0) { throw 'Fixture CLI did not report its version' }
        $launcherVersion = @(& $launcher --version)
        if ($LASTEXITCODE -ne 0 -or $launcherVersion.Count -ne 1 -or $launcherVersion[0] -cne $version) {
            throw 'The installed launcher did not start the fixture CLI'
        }
        $legacyVersion = @(& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $env:DIR 'dotfiles.ps1') --version)
        if ($LASTEXITCODE -ne 0 -or $legacyVersion.Count -ne 1 -or $legacyVersion[0] -cne $version) {
            throw 'The wrapper did not start the fixture CLI under Windows PowerShell'
        }
        Assert-ProfileStartup pwsh.exe $psProfile $launcher $version
        Assert-ProfileStartup powershell.exe $legacyProfile $launcher $version
        [System.IO.File]::WriteAllBytes(
            $gitConfigSnapshot,
            [System.IO.File]::ReadAllBytes($gitConfig)
        )
        [System.IO.File]::WriteAllBytes(
            $psProfileSnapshot,
            [System.IO.File]::ReadAllBytes($psProfile)
        )

        $gitConfigState = Get-LinkState $gitConfig
        $psProfileState = Get-LinkState $psProfile
        Write-Information "Running second install to check convergence..." -InformationAction Continue
        & $env:BINARY_PATH update --no-repo-update --root $env:DIR -p base --skip packages,agent-packages,vscode-extensions
        if ($LASTEXITCODE -ne 0) {
            throw "Second install command failed with exit code $LASTEXITCODE"
        }
        Assert-InstalledUnchanged $gitConfig $gitConfigSource $gitConfigSnapshot $gitConfigState
        Assert-InstalledUnchanged $psProfile $psProfileSource $psProfileSnapshot $psProfileState

        Write-Information "Running uninstall..." -InformationAction Continue
        & $env:BINARY_PATH remove --root $env:DIR -p base
        if ($LASTEXITCODE -ne 0) {
            throw "Uninstall command failed with exit code $LASTEXITCODE"
        }
        Write-Information "Uninstall complete" -InformationAction Continue

        Assert-Materialized $gitConfig $gitConfigSource $gitConfigSnapshot
        Assert-Materialized $psProfile $psProfileSource $psProfileSnapshot
        Assert-Materialized $legacyProfile $psProfileSource $psProfileSnapshot
        Assert-Materialized $terminal $terminalSource $terminalSnapshot
        if (Test-Path -LiteralPath $launcher) { throw 'Removal left the installed launcher' }
    }
    finally {
        Remove-Item -LiteralPath $gitConfigSnapshot, $psProfileSnapshot, $terminalSnapshot -Force -ErrorAction SilentlyContinue
    }
}

Test-InstallUninstallBaseProfile
Write-Information "`nAll install/uninstall tests passed" -InformationAction Continue
