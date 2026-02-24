<#
.SYNOPSIS
    PowerShell entry point for dotfiles management engine.
.DESCRIPTION
    Thin wrapper that downloads (or builds with -Build) the dotfiles Rust binary
    and forwards all arguments to it. Works on both Windows and Linux (pwsh).

    Default: downloads the latest published binary from GitHub Releases.
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
$CacheFile = Join-Path $BinDir ".dotfiles-version-cache"
$CacheMaxAge = 3600
$TransferTimeout = 120  # seconds — total transfer timeout
$RetryCount = 3         # number of download attempts
$RetryDelay = 2         # seconds between retries

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
        cargo build --profile dev-opt
        $BuildBinary = Join-Path $DotfilesRoot (Join-Path "cli" (Join-Path "target" (Join-Path "dev-opt" $BinaryName)))
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

# Production mode: ensure latest binary
function Get-LocalVersion
{
    if (Test-Path $Binary)
    {
        $output = & $Binary version 2>$null
        if ($output -match 'dotfiles\s+(.+)')
        {
            return $Matches[1]
        }
    }
    return "none"
}

function Get-LatestVersion
{
    try
    {
        $response = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -TimeoutSec $TransferTimeout
        return $response.tag_name
    }
    catch [System.Net.WebException]
    {
        Write-Verbose "Network error checking latest version: $($_.Exception.Message)"
        return ""
    }
    catch [System.Threading.Tasks.TaskCanceledException]
    {
        Write-Verbose "Timed out checking latest version"
        return ""
    }
    catch
    {
        Write-Verbose "Could not check latest version: $($_.Exception.Message)"
        return ""
    }
}

function Get-Binary
{
    param ([string]$Version)

    $url = "https://github.com/$Repo/releases/download/$Version/$AssetName"

    if (-not (Test-Path $BinDir))
    {
        New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    }

    Write-Output "Downloading dotfiles $Version..."
    $downloaded = $false
    for ($attempt = 1; $attempt -le $RetryCount; $attempt++)
    {
        if ($attempt -gt 1)
        {
            Write-Output "Retry $attempt/$RetryCount after ${RetryDelay}s..."
            Start-Sleep -Seconds $RetryDelay
        }
        try
        {
            Invoke-WebRequest -Uri $url -OutFile $Binary -UseBasicParsing -TimeoutSec $TransferTimeout
            $downloaded = $true
            break
        }
        catch [System.Net.WebException]
        {
            Write-Verbose "Download attempt $attempt failed (network): $($_.Exception.Message)"
        }
        catch [System.Threading.Tasks.TaskCanceledException]
        {
            Write-Verbose "Download attempt $attempt timed out"
        }
        catch
        {
            Write-Verbose "Download attempt $attempt failed: $($_.Exception.Message)"
        }
    }

    if (-not $downloaded)
    {
        if (Test-Path $Binary) { Remove-Item $Binary -Force }
        Write-Error "Failed to download dotfiles $Version after $RetryCount attempts. Check your internet connection or use -Build to build from source."
        exit 1
    }

    # Verify checksum
    $checksumUrl = "https://github.com/$Repo/releases/download/$Version/checksums.sha256"
    try
    {
        $checksums = (Invoke-WebRequest -Uri $checksumUrl -UseBasicParsing -TimeoutSec $TransferTimeout).Content
        $expected = ($checksums -split "`n" | Where-Object { $_ -match [regex]::Escape($AssetName) }) -replace '\s+.*', ''
        $actual = (Get-FileHash -Path $Binary -Algorithm SHA256).Hash.ToLower()
        if ($expected -and ($expected -ne $actual))
        {
            Remove-Item $Binary -Force
            Write-Error "Checksum verification failed!"
            exit 1
        }
    }
    catch
    {
        Write-Verbose "Could not verify checksum: $_"
    }

    # Make binary executable on Linux
    if ($IsLinux -or $IsMacOS)
    {
        chmod +x $Binary
    }
}

function Write-CacheFile
{
    param ([string]$Version)
    @($Version, [int][DateTimeOffset]::UtcNow.ToUnixTimeSeconds()) | Set-Content $CacheFile
}

function Get-CachedVersion
{
    if (Test-Path $CacheFile)
    {
        $lines = Get-Content $CacheFile
        if ($lines.Count -ge 1)
        {
            return $lines[0]
        }
    }
    return ""
}

function Test-CacheFresh
{
    if (-not (Test-Path $CacheFile))
    {
        return $false
    }
    $lines = Get-Content $CacheFile
    if ($lines.Count -lt 2)
    {
        return $false
    }
    $cachedTs = [int]$lines[1]
    $now = [int][DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    return (($now - $cachedTs) -lt $CacheMaxAge)
}

# Ensure binary is present and up to date
$localVersion = Get-LocalVersion

if (($localVersion -ne "none") -and (Test-CacheFresh))
{
    & $Binary --root $DotfilesRoot @CliArgs
    $ec = $LASTEXITCODE
    Wait-IfElevated
    exit $ec
}

$latest = Get-LatestVersion
if ([string]::IsNullOrEmpty($latest))
{
    if ($localVersion -ne "none")
    {
        Write-Output "Using cached dotfiles $localVersion (offline)"
        & $Binary --root $DotfilesRoot @CliArgs
        $ec = $LASTEXITCODE
        Wait-IfElevated
        exit $ec
    }
    Write-Error "Cannot determine latest version and no local binary found. Use -Build to build from source."
    exit 1
}

# Compare cached release tag (not binary's self-reported version) to avoid
# unnecessary re-downloads when git-describe output differs from release tag.
$cached = Get-CachedVersion
if (($localVersion -eq "none") -or ($cached -ne $latest))
{
    Get-Binary -Version $latest
}

Write-CacheFile -Version $latest
& $Binary --root $DotfilesRoot @CliArgs
$ec = $LASTEXITCODE
Wait-IfElevated
exit $ec
