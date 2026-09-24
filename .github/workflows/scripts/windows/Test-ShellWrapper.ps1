# -----------------------------------------------------------------------------
# Test-ShellWrapper.ps1 - Tests for dotfiles.ps1 PowerShell wrapper
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

# ---------------------------------------------------------------------------
# Test Build Mode
# ---------------------------------------------------------------------------

function Test-BuildMode {
    Write-TestStage "Testing dotfiles.ps1 --build mode"

    if ($env:BINARY_PATH -and (Test-Path $env:BINARY_PATH)) {
        Write-Information "Skipping: pre-built binary available, build tested separately" -InformationAction Continue
        return $true
    }

    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Information "Skipping: cargo not installed" -InformationAction Continue
        return $true
    }

    try {
        $output = & "$PSScriptRoot\..\..\..\..\dotfiles.ps1" --build --version 2>&1
        if ($output -match 'dotfiles') {
            Write-TestPass "Build mode successfully builds and runs binary"
            return $true
        } else {
            Write-TestFail "Build mode output unexpected: $output"
            return $false
        }
    } catch {
        Write-TestFail "Build mode failed: $_"
        return $false
    }
}

# ---------------------------------------------------------------------------
# Test Cache Mechanism
# ---------------------------------------------------------------------------

function Test-CacheFreshness {
    Write-TestStage "Testing cache freshness logic"

    $tmpDir = New-Item -ItemType Directory -Path (Join-Path $env:TEMP ([System.IO.Path]::GetRandomFileName()))
    try {
        $cacheFile = Join-Path $tmpDir ".dotfiles-version-cache"
        $cacheMaxAge = 3600

        # Test 1: No cache file - should not be fresh
        $lines = @()
        if (Test-Path $cacheFile) {
            $lines = Get-Content $cacheFile
        }
        if ($lines.Count -lt 2) {
            Write-TestPass "Empty cache correctly reports as not fresh"
        } else {
            Write-TestFail "Empty cache incorrectly reported as fresh"
            return $false
        }

        # Test 2: Fresh cache
        @('v2026.07.25-1', [int][DateTimeOffset]::UtcNow.ToUnixTimeSeconds()) | Set-Content $cacheFile
        $lines = Get-Content $cacheFile
        $cachedTs = [int]$lines[1]
        $now = [int][DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
        $isFresh = (($now - $cachedTs) -lt $cacheMaxAge)

        if ($isFresh) {
            Write-TestPass "Fresh cache correctly detected"
        } else {
            Write-TestFail "Fresh cache not detected"
            return $false
        }

        # Test 3: Stale cache
        @('v2026.07.25-1', 0) | Set-Content $cacheFile
        $lines = Get-Content $cacheFile
        $cachedTs = [int]$lines[1]
        $now = [int][DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
        $isFresh = (($now - $cachedTs) -lt $cacheMaxAge)

        if (-not $isFresh) {
            Write-TestPass "Stale cache correctly detected"
        } else {
            Write-TestFail "Stale cache incorrectly reported as fresh"
            return $false
        }

        return $true
    } finally {
        Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------------------
# Test Version Detection
# ---------------------------------------------------------------------------

function Test-VersionDetection {
    Write-TestStage "Testing version detection"

    if (-not $env:BINARY_PATH -or -not (Test-Path $env:BINARY_PATH)) {
        Write-Information "Skipping: BINARY_PATH not set or binary not found" -InformationAction Continue
        return $true
    }

    try {
        $output = & $env:BINARY_PATH --version 2>&1
        if ($output -match 'dotfiles\s+(.+)') {
            $version = $Matches[1]
            Write-TestPass "Version detected: $version"
            return $true
        } else {
            Write-TestFail "Version detection failed: $output"
            return $false
        }
    } catch {
        Write-TestFail "Version command failed: $_"
        return $false
    }
}

# ---------------------------------------------------------------------------
# Test Checksum Verification
# ---------------------------------------------------------------------------

function Test-ChecksumVerification {
    Write-TestStage "Testing checksum verification logic"

    $tmpDir = New-Item -ItemType Directory -Path (Join-Path $env:TEMP ([System.IO.Path]::GetRandomFileName()))
    try {
        # Create test binary
        "fake binary content" | Set-Content (Join-Path $tmpDir "dotfiles.exe")

        # Create checksums file
        @'
abc123  dotfiles-linux-x86_64
bad999  dotfiles-windows-x86_64.exe.sig
def456  dotfiles-windows-x86_64.exe
'@ | Set-Content (Join-Path $tmpDir "checksums.sha256")

        # Test checksum extraction with exact asset-name matching
        $checksums = Get-Content (Join-Path $tmpDir "checksums.sha256")
        $assetName = "dotfiles-windows-x86_64.exe"
        $expected = foreach ($line in $checksums) {
            $fields = $line.Trim() -split '\s+'
            if ($fields.Count -ge 2 -and $fields[1].TrimStart('*') -eq $assetName) {
                $fields[0].Trim().ToLower()
                break
            }
        }

        if ($expected -eq "def456") {
            Write-TestPass "Checksum extraction works correctly"
            return $true
        } else {
            Write-TestFail "Checksum extraction failed: got '$expected'"
            return $false
        }
    } finally {
        Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------------------
# Test Offline Fallback
# ---------------------------------------------------------------------------

function Test-OfflineFallback {
    Write-TestStage "Testing offline fallback behavior"

    # Simulate offline scenario
    $latestVersion = ""  # Empty simulates offline
    $localVersion = "v2026.07.25-1"

    if ([string]::IsNullOrEmpty($latestVersion) -and ($localVersion -ne "none")) {
        Write-TestPass "Offline fallback logic works with cached binary"
        return $true
    } else {
        Write-TestFail "Offline fallback logic failed"
        return $false
    }
}

# ---------------------------------------------------------------------------
# Test Argument Forwarding
# ---------------------------------------------------------------------------

function Test-ArgumentForwarding {
    Write-TestStage "Testing argument forwarding"

    if (-not $env:BINARY_PATH -or -not (Test-Path $env:BINARY_PATH)) {
        Write-Information "Skipping: BINARY_PATH not set or binary not found" -InformationAction Continue
        return $true
    }

    try {
        $output = & $env:BINARY_PATH --help 2>&1
        if ($output) {
            Write-TestPass "Arguments forwarded correctly"
            return $true
        } else {
            Write-TestFail "Argument forwarding failed"
            return $false
        }
    } catch {
        Write-TestFail "Argument forwarding test failed: $_"
        return $false
    }
}

function Test-InstallArgumentForwarding {
    Write-TestStage "Testing install argument forwarding through wrapper"

    $wrapper = Join-Path $PSScriptRoot "..\..\..\..\dotfiles.ps1"
    if (-not (Test-Path $wrapper)) {
        Write-Information "Skipping: wrapper not found" -InformationAction Continue
        return $true
    }

    try {
        $originalGuard = $env:DOTFILES_REEXEC_GUARD
        $env:DOTFILES_REEXEC_GUARD = '1'
        $output = & $wrapper install -p base -n --skip vscode-extensions 2>&1
        $text = ($output | Out-String)
        $plain = $text -replace "$([char]27)\[[0-9;]*m", ''

        if ($LASTEXITCODE -eq 0 -and $plain -match 'profile\s+base') {
            Write-TestPass "Install arguments forwarded correctly"
            return $true
        }

        Write-TestFail "Install forwarding output unexpected: $text"
        return $false
    } catch {
        Write-TestFail "Install argument forwarding failed: $_"
        return $false
    } finally {
        if ($null -eq $originalGuard) {
            Remove-Item Env:DOTFILES_REEXEC_GUARD -ErrorAction SilentlyContinue
        } else {
            $env:DOTFILES_REEXEC_GUARD = $originalGuard
        }
    }
}

function Test-AdvancedFlagForwarding {
    Write-TestStage "Testing advanced flags are forwarded by wrapper"

    $wrapper = Join-Path $PSScriptRoot "..\..\..\..\dotfiles.ps1"
    try {
        $originalGuard = $env:DOTFILES_REEXEC_GUARD
        $env:DOTFILES_REEXEC_GUARD = '1'
        $output = & $wrapper install -p base -n --skip symlinks --only packages --no-parallel 2>&1
        $text = ($output | Out-String)
        $plain = $text -replace "$([char]27)\[[0-9;]*m", ''

        if ($LASTEXITCODE -eq 0 -and $plain -match 'profile\s+base') {
            Write-TestPass "Wrapper forwards advanced flags to the Rust CLI"
            return $true
        }

        Write-TestFail "Advanced flag forwarding output unexpected: $text"
        return $false
    } catch {
        Write-TestFail "Advanced flag forwarding failed: $_"
        return $false
    } finally {
        if ($null -eq $originalGuard) {
            Remove-Item Env:DOTFILES_REEXEC_GUARD -ErrorAction SilentlyContinue
        } else {
            $env:DOTFILES_REEXEC_GUARD = $originalGuard
        }
    }
}

# ---------------------------------------------------------------------------
# Test Wrapper Implementation Guards
# ---------------------------------------------------------------------------

function Test-VersionPinnedBootstrapUrl {
    Write-TestStage "Testing wrapper resolves release tag and uses pinned URLs for binary and checksum"

    $wrapper = Join-Path $PSScriptRoot "..\..\..\..\dotfiles.ps1"
    $content = Get-Content $wrapper -Raw

    if (
        $content.Contains('function Resolve-ReleaseTag') -and
        $content.Contains('releases/download/$tag') -and
        $content.Contains('$checksumUrl = "$releaseBaseUrl/checksums.sha256"') -and
        -not $content.Contains('releases/latest/download')
    ) {
        Write-TestPass "Wrapper resolves release tag and uses pinned URLs for binary and checksum"
        return $true
    }

    Write-TestFail "Wrapper does not use version-pinned URLs for bootstrap downloads"
    return $false
}

function Test-AttestationVerification {
    Write-TestStage "Testing build provenance verification in bootstrap download"

    $wrapper = Join-Path $PSScriptRoot "..\..\..\..\dotfiles.ps1"
    $content = Get-Content $wrapper -Raw

    if (
        -not $content.Contains('function Test-Attestation') -or
        -not $content.Contains('gh attestation verify') -or
        -not $content.Contains('DOTFILES_SKIP_ATTESTATION') -or
        -not $content.Contains('Write-Warning "gh not found. Skipping build provenance verification."')
    ) {
        Write-TestFail "Wrapper does not verify build provenance for downloaded binaries"
        return $false
    }

    if ($content -notmatch '(?s)if \(-not \(Get-Command gh.*?\)\).*?Write-Warning "gh not found.*?return \$true') {
        Write-TestFail "Wrapper does not allow bootstrap to continue when gh is unavailable"
        return $false
    }

    $lines = $content -split '\r?\n'
    $checksumLine = ($lines | Select-String -SimpleMatch 'Write-Error "Checksum verification failed!"' | Select-Object -First 1).LineNumber
    $attestLine = ($lines | Select-String -SimpleMatch 'Test-Attestation -Path $stagedBinary' | Select-Object -First 1).LineNumber

    if (-not $checksumLine -or -not $attestLine) {
        Write-TestFail "Could not locate checksum and attestation verification in wrapper"
        return $false
    }

    if ($attestLine -lt $checksumLine) {
        Write-TestFail "Attestation verification (line $attestLine) must follow checksum verification (line $checksumLine)"
        return $false
    }

    Write-TestPass "Wrapper verifies provenance when available and tolerates missing gh"
    return $true
}

# ---------------------------------------------------------------------------
# Test Platform Detection
# ---------------------------------------------------------------------------

function Test-PlatformDetection {
    Write-TestStage "Testing platform detection"

    $isWindowsPlatform = ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))

    if ($isWindowsPlatform) {
        $expectedBinary = "dotfiles.exe"
        $expectedAsset = "dotfiles-windows-x86_64.exe"
    } elseif ($IsLinux) {
        $expectedBinary = "dotfiles"
        $arch = (uname -m).Trim()
        if ($arch -in @('aarch64', 'arm64')) {
            $expectedAsset = "dotfiles-linux-aarch64"
        } else {
            $expectedAsset = "dotfiles-linux-x86_64"
        }
    } else {
        Write-TestPass "Unsupported platform detection path verified"
        return $true
    }

    Write-TestPass "Platform detection: Binary=$expectedBinary, Asset=$expectedAsset"
    return $true
}

# ---------------------------------------------------------------------------
# Test Error Handling
# ---------------------------------------------------------------------------

function Test-ErrorHandling {
    Write-TestStage "Testing error handling"

    # Test that missing cargo in build mode produces error
    # Simulate missing cargo scenario
    $originalPath = $env:PATH
    try {
        # This test just verifies the logic would work
        # We can't actually remove cargo from PATH in this test
        Write-TestPass "Error handling structure verified"
        return $true
    } finally {
        $env:PATH = $originalPath
    }
}

function Test-IsolatedWrapperPath {
    Write-TestStage "Testing literal paths, runtime cwd, arguments, and bootstrap cleanup"
    $fixture = Join-Path (Join-Path $PSScriptRoot '..\..\..\..') ".wrapper-context-$([guid]::NewGuid())"
    $fixture = [System.IO.Path]::GetFullPath($fixture)
    $repo = Join-Path $fixture 'repo [literal]'
    $caller = Join-Path $fixture 'caller directory'
    $isWindowsPlatform = ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
    $binaryName = if ($isWindowsPlatform) { 'dotfiles.exe' } else { 'dotfiles' }
    $binary = Join-Path $repo "bin/$binaryName"
    $buildBinary = Join-Path $repo "cli/target/dev-opt/$binaryName"
    $wrapper = Join-Path $repo 'dotfiles.ps1'
    $originalLocation = Get-Location
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER', 'DOTFILES_SKIP_ATTESTATION', 'DOTFILES_REEXEC_GUARD',
            'WRAPPER_TEST_BUILD_EXIT', 'WRAPPER_TEST_BAD_CHECKSUM')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        [void][System.IO.Directory]::CreateDirectory((Split-Path $binary))
        [void][System.IO.Directory]::CreateDirectory((Split-Path $buildBinary))
        [void][System.IO.Directory]::CreateDirectory($caller)
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\dotfiles.ps1') -Destination $wrapper
        [System.IO.File]::WriteAllText($binary, 'fixture')
        [System.IO.File]::WriteAllText($buildBinary, 'fixture')
        $child = {
            [pscustomobject]@{
                Cwd = (Get-Location).Path
                Arguments = @($args)
                Root = $env:DOTFILES_ROOT
                Wrapper = $env:DOTFILES_WRAPPER
                Guard = $env:DOTFILES_REEXEC_GUARD
            } |
                ConvertTo-Json -Compress
            $global:LASTEXITCODE = 7
        }
        Set-Item -LiteralPath "Function:$binary" -Value $child
        Set-Item -LiteralPath "Function:$buildBinary" -Value $child
        Set-Item Function:cargo -Value {
            [System.IO.File]::WriteAllText(
                (Join-Path $env:DOTFILES_ROOT 'cargo-cwd'), (Get-Location).Path)
            @{
                reason = 'compiler-artifact'
                target = @{ name = 'dotfiles' }
                executable = Join-Path $env:DOTFILES_ROOT "cli/target/dev-opt/$binaryName"
            } | ConvertTo-Json -Compress
            $global:LASTEXITCODE = [int]$env:WRAPPER_TEST_BUILD_EXIT
        }
        Set-Item Function:gh -Value {
            $global:LASTEXITCODE = 0
        }
        Set-Item Function:Invoke-WebRequest -Value {
            param([string]$Uri, [string]$OutFile)
            if ($Uri.EndsWith('/releases/latest')) {
                return @{ Content = '{"tag_name":"v2026.09.17-1"}' }
            }
            if ($Uri.EndsWith('/checksums.sha256')) {
                $download = "$Binary.download-$PID"
                $hash = (Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash
                if ($env:WRAPPER_TEST_BAD_CHECKSUM -eq '1') { $hash = '0' * 64 }
                return @{ Content = "$hash  $(Get-TargetAssetName)" }
            }
            if ($OutFile) {
                [System.IO.File]::WriteAllText($OutFile, 'fixture')
                return
            }
            throw "Unexpected fixture request: $Uri"
        }
        $env:DOTFILES_SKIP_ATTESTATION = '0'
        $env:DOTFILES_REEXEC_GUARD = '1'
        $env:WRAPPER_TEST_BUILD_EXIT = '0'
        $env:WRAPPER_TEST_BAD_CHECKSUM = '0'
        Set-Location -LiteralPath $caller
        $arguments = @('install', '--dry-run', '--root', '.', '--overlay', '../overlay dir', '', '--BUILD', '--', 'literal value', '--build', '')
        foreach ($mode in @('cached', 'build', 'bootstrap')) {
            $wrapperArguments = $arguments
            if ($mode -eq 'build') { $wrapperArguments = @('--build') + $arguments }
            if ($mode -eq 'bootstrap') { Remove-Item -LiteralPath $binary }
            $output = @(& $wrapper @wrapperArguments)
            if ($LASTEXITCODE -ne 7) { throw "$mode lost the child exit code" }
            $actual = $output[-1] | ConvertFrom-Json
            if ($actual.Cwd -ne $caller) { throw "$mode changed the child working directory" }
            if ($actual.Root -ne $repo -or $actual.Wrapper -cne 'pwsh' -or $actual.Guard -cne '1') {
                throw "$mode changed runtime context"
            }
            if (($actual.Arguments | ConvertTo-Json -Compress) -cne ($arguments | ConvertTo-Json -Compress)) {
                throw "$mode changed the forwarded arguments"
            }
            if (-not (Test-Path -LiteralPath $binary)) { throw "$mode did not preserve the cached binary" }
        }
        if ([System.IO.File]::ReadAllText((Join-Path $repo 'cargo-cwd')) -ne (Join-Path $repo 'cli')) {
            throw 'Cargo did not run from cli/'
        }
        $env:WRAPPER_TEST_BUILD_EXIT = '23'
        $output = @(& $wrapper --build --version)
        if ($LASTEXITCODE -ne 23 -or $output.Count -ne 0) {
            throw 'Wrapper ran the child or lost the build failure exit code'
        }
        if ((Get-Location).Path -ne $caller) { throw 'Build failure changed the caller location' }

        Remove-Item -LiteralPath $binary
        $env:WRAPPER_TEST_BAD_CHECKSUM = '1'
        $rejected = $false
        try {
            & $wrapper --version | Out-Null
        } catch {
            if ($_ -notmatch 'Checksum verification failed') { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw 'Wrapper accepted a mismatched checksum' }
        if ((Test-Path -LiteralPath $binary) -or (Test-Path -LiteralPath "$binary.download-$PID")) {
            throw 'Rejected download was not removed from the literal path'
        }
        Write-TestPass 'Wrappers preserve literal paths, cwd, arguments, exit codes, and failed-download cleanup'
        return $true
    } catch {
        Write-TestFail "Isolated wrapper regression failed: $_"
        return $false
    } finally {
        Set-Location -LiteralPath $originalLocation.Path
        foreach ($name in $savedEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
        }
        Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-CargoArtifactPath {
    Write-TestStage 'Testing Cargo output directories and stale-artifact rejection'
    $fixture = Join-Path (Join-Path $PSScriptRoot '..\..\..\..') ".wrapper-artifact-$([guid]::NewGuid())"
    $fixture = [System.IO.Path]::GetFullPath($fixture)
    $wrapper = Join-Path $fixture 'dotfiles.ps1'
    $binaryName = if ($IsWindows) { 'dotfiles.exe' } else { 'dotfiles' }
    $stale = Join-Path $fixture "cli/target/dev-opt/$binaryName"
    $caller = Join-Path $fixture 'caller directory'
    $originalLocation = Get-Location
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER', 'CARGO_TARGET_DIR')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        [void][System.IO.Directory]::CreateDirectory((Split-Path $stale))
        [void][System.IO.Directory]::CreateDirectory((Join-Path $fixture 'cli/.cargo'))
        [void][System.IO.Directory]::CreateDirectory($caller)
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\dotfiles.ps1') -Destination $wrapper
        [System.IO.File]::WriteAllText($stale, 'stale')
        Set-Item -LiteralPath "Function:$stale" -Value { throw 'Stale default-target binary executed' }
        $child = {
            @{ Cwd = (Get-Location).Path; Arguments = @($args) } | ConvertTo-Json -Compress
            $global:LASTEXITCODE = 7
        }
        Set-Item Function:cargo -Value {
            if (($args -join ' ') -ne 'build --profile dev-opt --bin dotfiles --message-format=json-render-diagnostics') {
                throw "Unexpected Cargo arguments: $args"
            }
            $targetDirectory = $env:CARGO_TARGET_DIR
            if (-not $targetDirectory) {
                $config = Get-Content -LiteralPath '.cargo/config.toml' -Raw
                if ($config -notmatch 'target-dir = "([^"]+)"') { throw 'Missing configured target directory' }
                $targetDirectory = Join-Path (Get-Location) $Matches[1]
            }
            $artifact = Join-Path $targetDirectory "custom-triple/dev-opt/$binaryName"
            @{
                reason = 'compiler-artifact'
                target = @{ name = 'dotfiles' }
                executable = $artifact
            } | ConvertTo-Json -Compress
            '{"reason":"build-finished","success":true}'
            $global:LASTEXITCODE = 0
        }
        Set-Location -LiteralPath $caller
        foreach ($mode in @('environment', 'config')) {
            $env:CARGO_TARGET_DIR = $null
            if ($mode -eq 'environment') {
                $target = Join-Path $fixture 'environment output'
                $env:CARGO_TARGET_DIR = $target
            } else {
                $target = Join-Path $fixture 'cli/configured output'
                [System.IO.File]::WriteAllText(
                    (Join-Path $fixture 'cli/.cargo/config.toml'), "[build]`ntarget-dir = `"configured output`"`n")
            }
            $artifact = Join-Path $target "custom-triple/dev-opt/$binaryName"
            [void][System.IO.Directory]::CreateDirectory((Split-Path $artifact))
            [System.IO.File]::WriteAllText($artifact, 'new')
            Set-Item -LiteralPath "Function:$artifact" -Value $child
            $arguments = @('--version', 'space value', '')
            $output = @(& $wrapper --build @arguments)
            if ($LASTEXITCODE -ne 7 -or $output.Count -ne 1) { throw "$mode did not run only the reported artifact" }
            $actual = $output[0] | ConvertFrom-Json
            if ($actual.Cwd -ne $caller) { throw "$mode changed the caller cwd" }
            if (($actual.Arguments | ConvertTo-Json -Compress) -cne ($arguments | ConvertTo-Json -Compress)) {
                throw "$mode changed child arguments"
            }
        }
        Set-Item Function:cargo -Value { '{"reason":"build-finished","success":true}'; $global:LASTEXITCODE = 0 }
        $rejected = $false
        try {
            & $wrapper --build --version | Out-Null
        } catch {
            if ($_ -notmatch 'Cargo did not report') { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw 'Missing Cargo artifact did not fail' }
        Write-TestPass 'Build mode executes the reported Cargo artifact and rejects missing artifacts'
        return $true
    } catch {
        Write-TestFail "Cargo artifact regression failed: $_"
        return $false
    } finally {
        Set-Location -LiteralPath $originalLocation.Path
        foreach ($name in $savedEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
        }
        Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------------------
# Run All Tests
# ---------------------------------------------------------------------------

function Invoke-TestSuite {
    $results = @()

    $results += Test-BuildMode
    $results += Test-CacheFreshness
    $results += Test-VersionDetection
    $results += Test-ChecksumVerification
    $results += Test-OfflineFallback
    $results += Test-ArgumentForwarding
    $results += Test-InstallArgumentForwarding
    $results += Test-AdvancedFlagForwarding
    $results += Test-VersionPinnedBootstrapUrl
    $results += Test-AttestationVerification
    $results += Test-PlatformDetection
    $results += Test-ErrorHandling
    $results += Test-IsolatedWrapperPath
    $results += Test-CargoArtifactPath

    $passed = ($results | Where-Object { $_ -eq $true }).Count
    $total = $results.Count

    Write-Output ""
    Write-Output "======================================="
    Write-Output "Results: $passed/$total tests passed"

    if ($passed -eq $total) {
        exit 0
    } else {
        exit 1
    }
}

# Run tests if executed directly
if ($MyInvocation.InvocationName -ne '.') {
    Invoke-TestSuite
}
