<#
.SYNOPSIS
    PowerShell entry point for dotfiles management engine.
.DESCRIPTION
    Thin wrapper that downloads (or builds with -Build) the dotfiles Rust binary
    and forwards all other arguments to it. Works on both Windows and Linux (pwsh).

    Default: downloads the latest published binary from GitHub Releases if no
    binary is present, then runs it. The binary handles its own updates.
    -Build:  builds the Rust binary from source (requires cargo).
.EXAMPLE
    PS> .\dotfiles.ps1 install --profile base --dry-run --only symlinks
.EXAMPLE
    PS> .\dotfiles.ps1 -Build install --profile desktop
#>

$ErrorActionPreference = 'Stop'
$DotfilesRoot = $PSScriptRoot
$env:DOTFILES_ROOT = $DotfilesRoot
$env:DOTFILES_WRAPPER = "pwsh"
$Repo = "sneivandt/dotfiles"
$BinDir = Join-Path $DotfilesRoot "bin"
$TransferTimeout = 120  # seconds — total transfer timeout
$Build = $false
$CliArgs = @()
foreach ($arg in $args)
{
    if ($arg -in @('-Build', '--build'))
    {
        $Build = $true
        continue
    }

    $CliArgs += $arg
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
        Write-Error "cargo not found. Install Rust to use -Build mode."
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

function Get-Binary
{
    $tag = Resolve-ReleaseTag
    if (-not $tag)
    {
        Write-Error "Failed to resolve latest release tag. Check your internet connection or use -Build to build from source."
        exit 1
    }

    $releaseBaseUrl = "https://github.com/$Repo/releases/download/$tag"
    $url = "$releaseBaseUrl/$AssetName"

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
        Write-Error "Failed to download dotfiles binary. Check your internet connection or use -Build to build from source."
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
