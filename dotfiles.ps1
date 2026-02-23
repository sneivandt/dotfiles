<#
.SYNOPSIS
    PowerShell entry point for dotfiles management engine.
.DESCRIPTION
    Thin wrapper that downloads (or builds with -Build) the dotfiles Rust binary
    and forwards all arguments to it. Works on both Windows and Linux (pwsh).

    Default: downloads the latest published binary from GitHub Releases and
             delegates version management to the Rust bootstrap command.
    -Build:  builds the Rust binary from source (requires cargo).
.PARAMETER Action
    Subcommand to run: install, uninstall, test, or version.
.PARAMETER Build
    Build and run from source instead of using the published binary.
.PARAMETER ProfileName
    Profile to use (base, desktop).
.PARAMETER DryRun
    Preview changes without applying them.
.EXAMPLE
    PS> .\dotfiles.ps1 install -p base -d
.EXAMPLE
    PS> .\dotfiles.ps1 -Build install -p desktop
#>

[CmdletBinding()]
param (
    [Parameter(Position = 0)]
    [ValidateSet('install', 'uninstall', 'test', 'version')]
    [string]$Action,

    [switch]$Build,

    [ValidateSet('base', 'desktop')]
    [Alias('p')]
    [string]$ProfileName,

    [Alias('d')]
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$DotfilesRoot = $PSScriptRoot
$Repo = "sneivandt/dotfiles"
$BinDir = Join-Path $DotfilesRoot "bin"

# When running in an elevated window, pause before closing so the user can see output
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

# Build CLI arguments from declared parameters
$CliArgs = @()
if ($ProfileName) { $CliArgs += '--profile'; $CliArgs += $ProfileName }
if ($DryRun) { $CliArgs += '--dry-run' }
if ($VerbosePreference -ne 'SilentlyContinue') { $CliArgs += '--verbose' }
if ($Action) { $CliArgs += $Action }

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

# Auto-elevate to administrator on Windows when not in dry-run mode
if (-not $DryRun -and ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT')))
{
    $currentPrincipal = New-Object Security.Principal.WindowsPrincipal(
        [Security.Principal.WindowsIdentity]::GetCurrent()
    )
    $isAdmin = $currentPrincipal.IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator
    )
    if (-not $isAdmin)
    {
        Write-Output "Not running as administrator. Requesting elevation..."

        if ($PSVersionTable.PSEdition -eq 'Core')
        {
            $psExe = 'pwsh'
        }
        else
        {
            $psExe = 'powershell'
        }

        $scriptArgs = @('-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $PSCommandPath)
        if ($Build) { $scriptArgs += '-Build' }
        if ($ProfileName) { $scriptArgs += '-ProfileName'; $scriptArgs += $ProfileName }
        if ($DryRun) { $scriptArgs += '-DryRun' }
        if ($PSBoundParameters.ContainsKey('Verbose')) { $scriptArgs += '-Verbose' }
        if ($Action) { $scriptArgs += $Action }

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
                Write-Error "UAC elevation was cancelled. Administrator privileges are required. Use -d (dry-run) to preview changes."
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

# Build mode: build from source
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
        & $BuildBinary --root $DotfilesRoot @CliArgs
        $ec = $LASTEXITCODE
        Wait-IfElevated
        exit $ec
    }
    finally
    {
        Pop-Location
    }
}

# Production mode: initial bootstrap then delegate version management to Rust.
if (-not (Test-Path $Binary))
{
    if (-not (Test-Path $BinDir))
    {
        New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    }
    try
    {
        $latestResponse = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -TimeoutSec 120
        $latest = $latestResponse.tag_name
    }
    catch
    {
        Write-Error "Cannot reach GitHub for first-time setup. Use -Build to build from source."
        exit 1
    }
    Write-Output "Downloading dotfiles $latest..."
    $url = "https://github.com/$Repo/releases/download/$latest/$AssetName"
    Invoke-WebRequest -Uri $url -OutFile $Binary -UseBasicParsing -TimeoutSec 120
    if ($IsLinux -or $IsMacOS) { chmod +x $Binary }
}

# Delegate version checking, downloading, and cache management to Rust.
# On Windows, bootstrap may stage a .new binary that we rename here after it exits.
& $Binary --root $DotfilesRoot bootstrap --repo $Repo
$NewBinary = "$Binary.new"
if (Test-Path $NewBinary)
{
    Remove-Item $Binary -Force
    Rename-Item $NewBinary $Binary
}

& $Binary --root $DotfilesRoot @CliArgs
$ec = $LASTEXITCODE
Wait-IfElevated
exit $ec
