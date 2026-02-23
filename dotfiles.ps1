<#
.SYNOPSIS
    Minimal entry point for the dotfiles management engine.
.DESCRIPTION
    Handles -Build (cargo must exist before the binary does), Windows UAC
    elevation, and a one-time initial download.  Everything else — argument
    parsing, version management, help text — is handled by the Rust binary.
.EXAMPLE
    PS> .\dotfiles.ps1 install --profile base --dry-run
.EXAMPLE
    PS> .\dotfiles.ps1 -Build install
#>

param([switch]$Build)

$ErrorActionPreference = 'Stop'
$DotfilesRoot = $PSScriptRoot
$Repo = "sneivandt/dotfiles"
$BinDir = Join-Path $DotfilesRoot "bin"

# When running in an elevated window, pause before closing so the user can see output.
function Wait-IfElevated
{
    if ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
    {
        $principal = New-Object Security.Principal.WindowsPrincipal(
            [Security.Principal.WindowsIdentity]::GetCurrent()
        )
        if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))
        {
            Write-Output ""
            Read-Host "Press Enter to close"
        }
    }
}

# Platform detection
if ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
{
    $BinaryName = "dotfiles.exe"
    $AssetName = "dotfiles-windows-x86_64.exe"
}
else
{
    $BinaryName = "dotfiles"
    $arch = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { 'aarch64' } else { 'x86_64' }
    $AssetName = "dotfiles-linux-$arch"
}
$Binary = Join-Path $BinDir $BinaryName

# Auto-elevate on Windows (skip when --dry-run / -d is present in the forwarded args).
$IsDryRun = $args -contains '--dry-run' -or $args -contains '-d'
if (-not $IsDryRun -and ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT')))
{
    $principal = New-Object Security.Principal.WindowsPrincipal(
        [Security.Principal.WindowsIdentity]::GetCurrent()
    )
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))
    {
        Write-Output "Not running as administrator. Requesting elevation..."
        $psExe = if ($PSVersionTable.PSEdition -eq 'Core') { 'pwsh' } else { 'powershell' }
        $scriptArgs = @('-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath)
        if ($Build) { $scriptArgs += '-Build' }
        $scriptArgs += $args
        try
        {
            $process = Start-Process -FilePath $psExe -ArgumentList $scriptArgs -Verb RunAs -PassThru
            Write-Output "Elevated PowerShell window opened (PID: $($process.Id))."
            exit 0
        }
        catch [System.ComponentModel.Win32Exception]
        {
            if ($_.Exception.NativeErrorCode -eq 1223)
            {
                Write-Error "UAC elevation was cancelled. Use --dry-run to preview changes."
            }
            else
            {
                Write-Error "Failed to elevate: $($_.Exception.Message)"
            }
            exit 1
        }
        catch
        {
            Write-Error "Failed to start elevated process: $($_.Exception.Message)"
            exit 1
        }
    }
}

# -Build must be handled here: cargo is needed before the binary exists.
if ($Build)
{
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue))
    {
        Write-Error "cargo not found. Install Rust to use -Build mode."
        exit 1
    }
    Push-Location (Join-Path $DotfilesRoot "cli")
    try
    {
        cargo build --release
        $BuildBinary = Join-Path $DotfilesRoot (Join-Path "cli" (Join-Path "target" (Join-Path "release" $BinaryName)))
        & $BuildBinary --root $DotfilesRoot @args
        $ec = $LASTEXITCODE
        Wait-IfElevated
        exit $ec
    }
    finally
    {
        Pop-Location
    }
}

# First-time setup: minimal one-shot download when no binary exists yet.
# Subsequent version checks and updates are handled by `dotfiles bootstrap`.
if (-not (Test-Path $Binary))
{
    if (-not (Test-Path $BinDir)) { New-Item -ItemType Directory -Path $BinDir -Force | Out-Null }
    try
    {
        $latest = (Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -TimeoutSec 120).tag_name
    }
    catch
    {
        Write-Error "Cannot reach GitHub for first-time setup. Use -Build to build from source."
        exit 1
    }
    Write-Output "Downloading dotfiles $latest..."
    Invoke-WebRequest -Uri "https://github.com/$Repo/releases/download/$latest/$AssetName" `
        -OutFile $Binary -UseBasicParsing -TimeoutSec 120
    if ($IsLinux -or $IsMacOS) { chmod +x $Binary }
}

# Delegate version management to Rust.
# On Windows, bootstrap may stage a .new binary that we rename here after it exits.
& $Binary --root $DotfilesRoot bootstrap --repo $Repo
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$NewBinary = "$Binary.new"
if (Test-Path $NewBinary) { Remove-Item $Binary -Force; Rename-Item $NewBinary $Binary }

& $Binary --root $DotfilesRoot @args
$ec = $LASTEXITCODE
Wait-IfElevated
exit $ec
