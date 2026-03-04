<#
.SYNOPSIS
    PowerShell entry point for dotfiles management engine.
.DESCRIPTION
    Thin wrapper that downloads (or builds with -Build) the dotfiles Rust binary
    and forwards all arguments to it. Works on both Windows and Linux (pwsh).

    Default: downloads the latest published binary from GitHub Releases if no
    binary is present, then runs it. The binary handles its own updates.
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
$ConnectTimeout = 10    # seconds — TCP connect timeout (used where supported)
$TransferTimeout = 120  # seconds — total transfer timeout
$RetryCount = 3         # number of download attempts
$RetryDelay = 2         # seconds between retries
# NOTE: Keep these constants in sync with the equivalent values in dotfiles.sh.
# dotfiles.sh: CONNECT_TIMEOUT / TRANSFER_TIMEOUT / RETRY_COUNT / RETRY_DELAY

# Build CLI arguments from declared parameters
$CliArgs = @()
if ($Action) { $CliArgs += $Action }
if ($ProfileName) { $CliArgs += '--profile'; $CliArgs += $ProfileName }
if ($DryRun) { $CliArgs += '--dry-run' }
if ($VerbosePreference -ne 'SilentlyContinue') { $CliArgs += '--verbose' }

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
        exit $LASTEXITCODE
    }
    finally
    {
        Pop-Location
    }
}

# Production mode: bootstrap binary if not present.
# Subsequent updates are handled by the binary itself.

# Use -ConnectionTimeoutSeconds if available (PowerShell 7.4+); computed once for all web requests.
$script:ConnectArgs = if ($PSVersionTable.PSVersion -ge [version]'7.4') {
    @{ ConnectionTimeoutSeconds = $ConnectTimeout }
} else {
    @{}
}

function Get-LatestVersion
{
    try
    {
        $response = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -TimeoutSec $TransferTimeout @script:ConnectArgs
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
            Invoke-WebRequest -Uri $url -OutFile $Binary -UseBasicParsing -TimeoutSec $TransferTimeout @script:ConnectArgs
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

    # Download and verify checksum
    $checksumUrl = "https://github.com/$Repo/releases/download/$Version/checksums.sha256"
    try
    {
        $checksumResponse = Invoke-WebRequest -Uri $checksumUrl -UseBasicParsing -TimeoutSec $TransferTimeout @script:ConnectArgs
        $checksumContent = if ($checksumResponse.Content -is [byte[]])
        {
            [System.Text.Encoding]::UTF8.GetString($checksumResponse.Content)
        }
        else
        {
            $checksumResponse.Content
        }
    }
    catch
    {
        if (Test-Path $Binary) { Remove-Item $Binary -Force }
        Write-Error "Failed to download checksum file for ${Version}: $($_.Exception.Message)"
        exit 1
    }
    $expectedLine = ($checksumContent -split "`n" | Where-Object { $_ -match [regex]::Escape($AssetName) } | Select-Object -First 1)
    if ([string]::IsNullOrWhiteSpace($expectedLine))
    {
        if (Test-Path $Binary) { Remove-Item $Binary -Force }
        Write-Error "Checksum not found in checksum file for $AssetName."
        exit 1
    }
    $expected = ($expectedLine -split '\s+')[0].Trim().ToLower()
    $actual = (Get-FileHash -Path $Binary -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual)
    {
        if (Test-Path $Binary) { Remove-Item $Binary -Force }
        Write-Error "Checksum verification failed!"
        exit 1
    }

    # Make binary executable on Linux
    if ($IsLinux -or $IsMacOS)
    {
        chmod +x $Binary
    }
}

# Bootstrap: download the latest binary only if no binary is present.
if (-not (Test-Path $Binary))
{
    $latest = Get-LatestVersion
    if ([string]::IsNullOrEmpty($latest))
    {
        Write-Error "Cannot determine latest version and no local binary found. Use -Build to build from source."
        exit 1
    }
    Get-Binary -Version $latest
}

& $Binary --root $DotfilesRoot @CliArgs
exit $LASTEXITCODE