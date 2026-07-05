<#
.SYNOPSIS
    PowerShell entry point for dotfiles management engine.
.DESCRIPTION
    Thin wrapper that downloads (or builds with --build) the dotfiles Rust binary
    and forwards all other arguments to it. Works on both Windows and Linux (pwsh).

    Default: downloads the latest published binary from GitHub Releases if no
    binary is present, then runs it. The binary handles its own updates.
    --build: builds the Rust binary from source (requires cargo).
.EXAMPLE
    PS> .\dotfiles.ps1 install --profile base --dry-run --only symlinks
.EXAMPLE
    PS> .\dotfiles.ps1 update --profile desktop
.EXAMPLE
    PS> .\dotfiles.ps1 --build install --profile desktop
#>

$ErrorActionPreference = 'Stop'
$DotfilesRoot = $PSScriptRoot -replace '^\\\\\?\\', ''
$env:DOTFILES_ROOT = $DotfilesRoot
$env:DOTFILES_WRAPPER = "pwsh"
$Repo = "sneivandt/dotfiles"
$BinDir = Join-Path $DotfilesRoot "bin"
$TransferTimeout = 120  # seconds — total transfer timeout
$Build = $false
$CliArgs = @()
foreach ($arg in $args)
{
    if ($arg -eq '--build')
    {
        $Build = $true
        continue
    }

    $CliArgs += $arg
}

function Test-IsWindows
{
    return ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
}

function Get-BinaryName
{
    if (Test-IsWindows)
    {
        return "dotfiles.exe"
    }

    if ($IsLinux)
    {
        return "dotfiles"
    }

    Write-Error "Unsupported operating system. Supported operating systems: Windows, Linux."
    exit 1
}

function Get-TargetAssetName
{
    if (Test-IsWindows)
    {
        $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
        switch ($arch)
        {
            { $_ -in @('AMD64', 'x86_64') } {
                return "dotfiles-windows-x86_64.exe"
            }
            default {
                Write-Error "Unsupported Windows architecture: $arch. Supported architectures: AMD64, x86_64."
                exit 1
            }
        }
    }

    if (-not $IsLinux)
    {
        Write-Error "Unsupported operating system. Supported operating systems: Windows, Linux."
        exit 1
    }

    $arch = (uname -m).Trim()
    switch ($arch)
    {
        { $_ -in @('x86_64', 'amd64') } {
            return "dotfiles-linux-x86_64"
        }
        { $_ -in @('aarch64', 'arm64') } {
            return "dotfiles-linux-aarch64"
        }
        default {
            Write-Error "Unsupported Linux architecture: $arch. Supported architectures: x86_64, amd64, aarch64, arm64."
            exit 1
        }
    }
}

$BinaryName = Get-BinaryName
$Binary = Join-Path $BinDir $BinaryName
$PendingBinary = Join-Path $BinDir ".dotfiles-update.pending"
$PendingVersion = Join-Path $BinDir ".dotfiles-update.version"

function Install-PendingBinary
{
    if (-not (Test-Path $PendingBinary))
    {
        return
    }

    if (-not (Test-Path $BinDir))
    {
        New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    }

    $backupBinary = Join-Path $BinDir ".dotfiles-binary.backup"
    $hadExistingBinary = Test-Path $Binary

    if (Test-Path $backupBinary)
    {
        Remove-Item $backupBinary -Force
    }

    try
    {
        if ($hadExistingBinary)
        {
            Move-Item -Path $Binary -Destination $backupBinary -Force
        }

        Move-Item -Path $PendingBinary -Destination $Binary -Force

        if (Test-Path $backupBinary)
        {
            Remove-Item $backupBinary -Force
        }

        if (Test-Path $PendingVersion)
        {
            Remove-Item $PendingVersion -Force
        }
    }
    catch
    {
        $rollbackError = $null

        if (Test-Path $Binary)
        {
            Remove-Item $Binary -Force -ErrorAction SilentlyContinue
        }

        if ($hadExistingBinary -and (Test-Path $backupBinary))
        {
            try
            {
                Move-Item -Path $backupBinary -Destination $Binary -Force
            }
            catch
            {
                $rollbackError = $_.Exception.Message
            }
        }

        $message = "Failed to promote downloaded dotfiles binary: $($_.Exception.Message)"
        if ($rollbackError)
        {
            $message += " Rollback failed: $rollbackError"
        }

        throw $message
    }
    finally
    {
        if ((Test-Path $backupBinary) -and (Test-Path $Binary))
        {
            Remove-Item $backupBinary -Force -ErrorAction SilentlyContinue
        }
    }
}

function Invoke-PendingBinaryInstallOrExit
{
    try
    {
        Install-PendingBinary
    }
    catch
    {
        Write-Error $_.Exception.Message
        exit 1
    }
}

# Build mode: build from source
if ($Build)
{
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue))
    {
        Write-Error "cargo not found. Install Rust to use --build mode."
        exit 1
    }
    Push-Location (Join-Path $DotfilesRoot "cli")
    try
    {
        cargo build --profile dev-opt
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        $BuildBinary = Join-Path $DotfilesRoot (Join-Path "cli" (Join-Path "target" (Join-Path "dev-opt" $BinaryName)))
        & $BuildBinary @CliArgs
        exit $LASTEXITCODE
    }
    finally
    {
        Pop-Location
    }
}

# Production mode: bootstrap binary if not present.
# Subsequent update checks are handled by the binary itself; this wrapper also
# promotes any staged Windows update before relaunch.

function Resolve-ReleaseTag
{
    $url = "https://api.github.com/repos/$Repo/releases/latest"
    try
    {
        $response = Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec $TransferTimeout
        $json = $response.Content | ConvertFrom-Json
        return $json.tag_name
    }
    catch
    {
        return $null
    }
}

function Get-ChecksumForAsset
{
    param(
        [Parameter(Mandatory = $true)]
        [string]$ChecksumContent,
        [Parameter(Mandatory = $true)]
        [string]$AssetName
    )

    foreach ($line in ($ChecksumContent -split '\r?\n'))
    {
        $fields = $line.Trim() -split '\s+'
        if ($fields.Count -lt 2)
        {
            continue
        }

        $name = $fields[1].TrimStart('*')
        if ($name -eq $AssetName)
        {
            return $fields[0].Trim().ToLower()
        }
    }

    return $null
}

function Get-Binary
{
    $tag = Resolve-ReleaseTag
    if (-not $tag)
    {
        Write-Error "Failed to resolve latest release tag. Check your internet connection or use --build to build from source."
        exit 1
    }

    $releaseBaseUrl = "https://github.com/$Repo/releases/download/$tag"
    $assetName = Get-TargetAssetName
    $url = "$releaseBaseUrl/$assetName"

    if (-not (Test-Path $BinDir))
    {
        New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
    }

    Write-Output "Downloading dotfiles bootstrap binary..."
    try
    {
        Invoke-WebRequest -Uri $url -Method Get -OutFile $Binary -UseBasicParsing -TimeoutSec $TransferTimeout | Out-Null
    }
    catch
    {
        if (Test-Path $Binary) { Remove-Item $Binary -Force }
        Write-Error "Failed to download dotfiles binary. Check your internet connection or use --build to build from source."
        exit 1
    }

    if (-not (Test-Path $Binary))
    {
        Write-Error "Download did not produce a binary at '$Binary'. Check your internet connection or use --build to build from source."
        exit 1
    }

    # Download and verify checksum
    $checksumUrl = "$releaseBaseUrl/checksums.sha256"
    try
    {
        $checksumResponse = Invoke-WebRequest -Uri $checksumUrl -Method Get -UseBasicParsing -TimeoutSec $TransferTimeout
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
        Write-Error "Failed to download checksum file: $($_.Exception.Message)"
        exit 1
    }
    $expected = Get-ChecksumForAsset -ChecksumContent $checksumContent -AssetName $assetName
    if ([string]::IsNullOrWhiteSpace($expected))
    {
        if (Test-Path $Binary) { Remove-Item $Binary -Force }
        Write-Error "Checksum not found in checksum file for $assetName."
        exit 1
    }
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
        if ($LASTEXITCODE -ne 0)
        {
            Write-Error "Failed to make binary executable"
            exit 1
        }
    }
}

# Bootstrap: download the latest binary only if no binary is present.
Invoke-PendingBinaryInstallOrExit

if (-not (Test-Path $Binary))
{
    Get-Binary
}

& $Binary @CliArgs
exit $LASTEXITCODE
