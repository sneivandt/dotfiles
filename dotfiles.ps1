# dotfiles.ps1 — Minimal entry point for the dotfiles management engine.
# Handles -Build and a one-time initial download; everything else is the Rust binary.

param([switch]$Build)

$ErrorActionPreference = 'Stop'
$DotfilesRoot = $PSScriptRoot
$Repo = "sneivandt/dotfiles"
$BinDir = Join-Path $DotfilesRoot "bin"

if ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT')) {
    $BinaryName = "dotfiles.exe"; $AssetName = "dotfiles-windows-x86_64.exe"
} else {
    $BinaryName = "dotfiles"
    $Arch = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { 'aarch64' } else { 'x86_64' }
    $AssetName = "dotfiles-linux-$Arch"
}
$Binary = Join-Path $BinDir $BinaryName

# -Build must be handled here: cargo is needed before the binary exists.
if ($Build) {
    Set-Location (Join-Path $DotfilesRoot "cli")
    cargo build --release
    & (Join-Path $DotfilesRoot "cli" "target" "release" $BinaryName) --root $DotfilesRoot @args
    exit $LASTEXITCODE
}

# First-time setup: one-shot download; subsequent updates handled by bootstrap.
if (-not (Test-Path $Binary)) {
    New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    $Latest = (Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -TimeoutSec 120).tag_name
    $Base = "https://github.com/$Repo/releases/download/$Latest"
    Invoke-WebRequest "$Base/$AssetName" -OutFile $Binary -UseBasicParsing -TimeoutSec 120
    $Checksums = try { (Invoke-WebRequest "$Base/checksums.sha256" -UseBasicParsing -TimeoutSec 120).Content } catch { $null }
    if ($Checksums) {
        $Expected = ($Checksums -split "`n" | Where-Object { $_ -match $AssetName }) -replace '\s+.*'
        if ($Expected) {
            $Actual = (Get-FileHash $Binary -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($Expected -ne $Actual) { Remove-Item $Binary -Force; throw "Checksum mismatch" }
        }
    }
    if ($IsLinux -or $IsMacOS) { chmod +x $Binary }
}

& $Binary --root $DotfilesRoot bootstrap --repo $Repo
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$NewBinary = "$Binary.new"
if (Test-Path $NewBinary) { Remove-Item $Binary -Force; Rename-Item $NewBinary $Binary }

& $Binary --root $DotfilesRoot @args
exit $LASTEXITCODE
