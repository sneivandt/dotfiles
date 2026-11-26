#Requires -Version 7.0
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z]+)*$')]
    [string]$Version
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Save-ReleaseFile {
    param(
        [Parameter(Mandatory)][string]$Uri,
        [Parameter(Mandatory)][string]$OutFile,
        [int]$Attempts = 3
    )

    for ($attempt = 1; $attempt -le $Attempts; $attempt++) {
        try {
            Invoke-WebRequest -Uri $Uri -OutFile $OutFile
            return
        } catch {
            if ($attempt -eq $Attempts) {
                throw
            }
            Write-Warning "Download attempt $attempt failed for ${Uri}: $($_.Exception.Message)"
            Start-Sleep -Seconds (2 * $attempt)
        }
    }
}

$archiveName = 'apm-windows-x86_64.zip'
$baseUrl = "https://github.com/microsoft/apm/releases/download/v$Version"
$tempRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }
$workDir = Join-Path $tempRoot "apm-$Version"
$archivePath = Join-Path $workDir $archiveName
$checksumPath = Join-Path $workDir "$archiveName.sha256"
$extractDir = Join-Path $workDir 'extract'

New-Item -ItemType Directory -Path $workDir -Force | Out-Null
Save-ReleaseFile -Uri "$baseUrl/$archiveName" -OutFile $archivePath
Save-ReleaseFile -Uri "$baseUrl/$archiveName.sha256" -OutFile $checksumPath

$checksumText = (Get-Content -LiteralPath $checksumPath -Raw).Trim()
if ($checksumText -notmatch '^([0-9a-fA-F]{64})\b') {
    throw "Unexpected SHA-256 file format from ${baseUrl}/${archiveName}.sha256: $checksumText"
}
$expectedHash = $Matches[1].ToLowerInvariant()
$actualHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualHash -ne $expectedHash) {
    throw "SHA-256 mismatch for ${archiveName}: expected $expectedHash, got $actualHash"
}

if (Test-Path -LiteralPath $extractDir) {
    Remove-Item -LiteralPath $extractDir -Recurse -Force
}
Expand-Archive -LiteralPath $archivePath -DestinationPath $extractDir -Force

$binDir = Join-Path $extractDir 'apm-windows-x86_64'
$apmExe = Join-Path $binDir 'apm.exe'
if (-not (Test-Path -LiteralPath $apmExe -PathType Leaf)) {
    throw "Extracted APM archive did not contain $apmExe"
}

& $apmExe --version
if ($LASTEXITCODE -ne 0) {
    throw "apm --version failed with exit code $LASTEXITCODE"
}

if ($env:GITHUB_PATH) {
    Add-Content -LiteralPath $env:GITHUB_PATH -Value $binDir
}
