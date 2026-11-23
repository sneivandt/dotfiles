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
    Write-TestStage "Testing the wrapper's checksum asset selection"
    try {
        $wrapper = Join-Path $PSScriptRoot '../../../../dotfiles.ps1'
        $tokens = $null
        $errors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile(
            $wrapper, [ref]$tokens, [ref]$errors)
        if ($errors.Count -ne 0) { throw "Wrapper parse failed: $errors" }
        $selector = $ast.Find({ param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq 'Get-ChecksumForAsset'
        }, $true)
        if (-not $selector) { throw 'Wrapper checksum selector is missing' }
        . ([scriptblock]::Create($selector.Extent.Text))
        foreach ($case in @(
            @{ Name = 'exact asset'; Content = "abc123  dotfiles-linux-x86_64`nbad999  dotfiles-windows-x86_64.exe.sig`ndef456  dotfiles-windows-x86_64.exe"; Expected = 'def456' },
            @{ Name = 'binary marker and CRLF'; Content = "invalid`r`n`tDEF456`t*dotfiles-windows-x86_64.exe`r`n"; Expected = 'def456' },
            @{ Name = 'only a suffix match'; Content = 'bad999  dotfiles-windows-x86_64.exe.sig'; Expected = $null },
            @{ Name = 'different platform'; Content = 'abc123  dotfiles-linux-x86_64'; Expected = $null },
            @{ Name = 'malformed lines'; Content = "single-field`n `n"; Expected = $null }
        )) {
            $actual = Get-ChecksumForAsset -ChecksumContent $case.Content -AssetName 'dotfiles-windows-x86_64.exe'
            if ($actual -cne $case.Expected) {
                throw "$($case.Name): expected '$($case.Expected)', got '$actual'"
            }
        }
        Write-TestPass 'Real checksum selector matches exact assets and rejects missing checksums'
        return $true
    } catch {
        Write-TestFail "Checksum selection regression failed: $_"
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

function Test-UpdateArgumentForwarding {
    Write-TestStage "Testing update argument forwarding through wrapper"

    $wrapper = Join-Path $PSScriptRoot "..\..\..\..\dotfiles.ps1"
    if (-not (Test-Path $wrapper)) {
        Write-Information "Skipping: wrapper not found" -InformationAction Continue
        return $true
    }

    try {
        $originalSkipSelfUpdate = $env:DOTFILES_SKIP_SELF_UPDATE
        $env:DOTFILES_SKIP_SELF_UPDATE = '1'
        $output = & $wrapper update --no-repo-update -p base -n --skip 'agent-packages,vscode-extensions' 2>&1
        $text = ($output | Out-String)
        $plain = $text -replace "$([char]27)\[[0-9;]*m", ''

        if ($LASTEXITCODE -eq 0 -and $plain -match 'base\s+profile') {
            Write-TestPass "Update arguments forwarded correctly"
            return $true
        }

        Write-TestFail "Update forwarding output unexpected: $text"
        return $false
    } catch {
        Write-TestFail "Update argument forwarding failed: $_"
        return $false
    } finally {
        if ($null -eq $originalSkipSelfUpdate) {
            Remove-Item Env:DOTFILES_SKIP_SELF_UPDATE -ErrorAction SilentlyContinue
        } else {
            $env:DOTFILES_SKIP_SELF_UPDATE = $originalSkipSelfUpdate
        }
    }
}

function Test-AdvancedFlagForwarding {
    Write-TestStage "Testing advanced flags are forwarded by wrapper"

    $wrapper = Join-Path $PSScriptRoot "..\..\..\..\dotfiles.ps1"
    try {
        $originalSkipSelfUpdate = $env:DOTFILES_SKIP_SELF_UPDATE
        $env:DOTFILES_SKIP_SELF_UPDATE = '1'
        $output = & $wrapper update -p base -n --skip symlinks --only packages --no-parallel 2>&1
        $text = ($output | Out-String)
        $plain = $text -replace "$([char]27)\[[0-9;]*m", ''

        if ($LASTEXITCODE -eq 0 -and $plain -match 'base\s+profile') {
            Write-TestPass "Wrapper forwards advanced flags to the Rust CLI"
            return $true
        }

        Write-TestFail "Advanced flag forwarding output unexpected: $text"
        return $false
    } catch {
        Write-TestFail "Advanced flag forwarding failed: $_"
        return $false
    } finally {
        if ($null -eq $originalSkipSelfUpdate) {
            Remove-Item Env:DOTFILES_SKIP_SELF_UPDATE -ErrorAction SilentlyContinue
        } else {
            $env:DOTFILES_SKIP_SELF_UPDATE = $originalSkipSelfUpdate
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

function Write-NativeStderrShim {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Directory,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )
    $isWindowsPlatform = ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
    $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
    if ($isWindowsPlatform) {
        $shimPath = Join-Path $Directory "$Name.cmd"
        $script = @(
            '@echo off'
            'if defined WRAPPER_TEST_NATIVE_MARKER echo invoked>"%WRAPPER_TEST_NATIVE_MARKER%"'
            '>&2 echo native stderr diagnostic'
            'if defined WRAPPER_TEST_NATIVE_STDOUT echo %WRAPPER_TEST_NATIVE_STDOUT%'
            'exit /b %WRAPPER_TEST_NATIVE_EXIT%'
        ) -join "`r`n"
        [System.IO.File]::WriteAllText($shimPath, "$script`r`n", $utf8NoBom)
        return $shimPath
    }
    $shimPath = Join-Path $Directory $Name
    $script = @(
        '#!/bin/sh'
        'if [ -n "${WRAPPER_TEST_NATIVE_MARKER:-}" ]; then printf "invoked\n" > "$WRAPPER_TEST_NATIVE_MARKER"; fi'
        'printf "native stderr diagnostic\n" >&2'
        'if [ -n "${WRAPPER_TEST_NATIVE_STDOUT:-}" ]; then printf "%s\n" "$WRAPPER_TEST_NATIVE_STDOUT"; fi'
        'exit "${WRAPPER_TEST_NATIVE_EXIT:-0}"'
    ) -join "`n"
    [System.IO.File]::WriteAllText($shimPath, "$script`n", $utf8NoBom)
    & chmod +x $shimPath
    if ($LASTEXITCODE -ne 0) { throw "Failed to mark native shim executable: $shimPath" }
    return $shimPath
}

function Test-AttestationVerification {
    Write-TestStage "Testing build provenance verification in bootstrap download"

    $wrapper = Join-Path $PSScriptRoot "..\..\..\..\dotfiles.ps1"
    $content = Get-Content $wrapper -Raw

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

    $tokens = $null
    $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile($wrapper, [ref]$tokens, [ref]$errors)
    if ($errors.Count) { throw "Wrapper parse failed: $errors" }
    $verifier = $ast.Find({ param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Test-Attestation'
    }, $true)
    if (-not $verifier) { throw "Wrapper has no attestation verifier" }
    . ([scriptblock]::Create($verifier.Extent.Text))
    $isWindowsPlatform = ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
    $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
    $savedEnvironment = @{}
    foreach ($name in @(
        'DOTFILES_SKIP_ATTESTATION', 'PATH', 'GH_PROMPT_DISABLED', 'DBUS_SESSION_BUS_ADDRESS',
        'WRAPPER_TEST_NATIVE_EXIT', 'WRAPPER_TEST_GH_CAPTURE'
    )) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    $savedStderr = [Console]::Error
    $stderr = [System.IO.StringWriter]::new()
    $nativeFixture = [System.IO.Path]::GetFullPath(
        (Join-Path (Join-Path $PSScriptRoot '../../../..') ".wrapper-native-gh-$([guid]::NewGuid())"))
    try {
        [Console]::SetError($stderr)

        $emptyDir = Join-Path $nativeFixture 'empty'
        $shimDir = Join-Path $nativeFixture 'bin'
        $brokenDir = Join-Path $nativeFixture 'broken'
        [void][System.IO.Directory]::CreateDirectory($emptyDir)
        [void][System.IO.Directory]::CreateDirectory($shimDir)
        [void][System.IO.Directory]::CreateDirectory($brokenDir)

        # A PowerShell function named gh that emits a nonterminating error without
        # touching $LASTEXITCODE must never be invoked or inherit a prior exit code 0.
        $script:functionGhInvoked = $false
        Set-Item -LiteralPath 'Function:global:gh' -Value {
            $script:functionGhInvoked = $true
            Write-Error 'PowerShell gh shim error'
        }

        $env:PATH = $emptyDir
        $env:DOTFILES_SKIP_ATTESTATION = '0'
        $global:LASTEXITCODE = 0
        if (Test-Attestation -Path 'fixture asset' -Repo 'fixture/releases') {
            throw 'Missing native gh accepted an unattested asset via Function:gh or stale LASTEXITCODE'
        }
        if ($script:functionGhInvoked) {
            throw 'Test-Attestation invoked a PowerShell function instead of a native gh application'
        }
        if (-not $stderr.ToString().Contains('WARNING: gh not found. Install GitHub CLI')) {
            throw 'Missing gh did not explain the refusal on stderr'
        }

        # Explicit bypass must succeed without invoking any gh command.
        $env:DOTFILES_SKIP_ATTESTATION = '1'
        if (-not (Test-Attestation -Path 'fixture asset' -Repo 'fixture/releases')) {
            throw 'Explicit bypass did not accept the asset'
        }
        if ($script:functionGhInvoked) {
            throw 'Explicit bypass invoked Function:gh'
        }
        $env:PATH = $savedEnvironment['PATH']

        # Real native gh shim writing to stderr, recording args and env vars,
        # while Function:gh is still defined to prove the native app wins.
        $captureFile = Join-Path $nativeFixture 'gh-capture.txt'
        if ($isWindowsPlatform) {
            $ghShim = Join-Path $shimDir 'gh.cmd'
            $shimLines = @(
                '@echo off'
                '('
                '  echo PROMPT=%GH_PROMPT_DISABLED%'
                '  echo DBUS=%DBUS_SESSION_BUS_ADDRESS%'
                '  echo ARG1=%~1'
                '  echo ARG2=%~2'
                '  echo ARG3=%~3'
                '  echo ARG4=%~4'
                '  echo ARG5=%~5'
                ')>"%WRAPPER_TEST_GH_CAPTURE%"'
                '>&2 echo native stderr diagnostic'
                'exit /b %WRAPPER_TEST_NATIVE_EXIT%'
            ) -join "`r`n"
            [System.IO.File]::WriteAllText($ghShim, "$shimLines`r`n", $utf8NoBom)
        } else {
            $ghShim = Join-Path $shimDir 'gh'
            $shimLines = @(
                '#!/bin/sh'
                '{'
                '  printf "PROMPT=%s\n" "${GH_PROMPT_DISABLED:-}"'
                '  printf "DBUS=%s\n" "${DBUS_SESSION_BUS_ADDRESS:-}"'
                '  printf "ARG1=%s\n" "${1:-}"'
                '  printf "ARG2=%s\n" "${2:-}"'
                '  printf "ARG3=%s\n" "${3:-}"'
                '  printf "ARG4=%s\n" "${4:-}"'
                '  printf "ARG5=%s\n" "${5:-}"'
                '} > "$WRAPPER_TEST_GH_CAPTURE"'
                'printf "native stderr diagnostic\n" >&2'
                'exit "${WRAPPER_TEST_NATIVE_EXIT:-0}"'
            ) -join "`n"
            [System.IO.File]::WriteAllText($ghShim, "$shimLines`n", $utf8NoBom)
            & chmod +x $ghShim
            if ($LASTEXITCODE -ne 0) { throw "Failed to mark native gh shim executable: $ghShim" }
        }

        $env:PATH = "$shimDir$([System.IO.Path]::PathSeparator)$($savedEnvironment['PATH'])"
        $resolvedGh = Get-Command gh -CommandType Application -ErrorAction Stop | Select-Object -First 1
        if ($resolvedGh.CommandType -ne 'Application' -or $resolvedGh.Source -ne $ghShim) {
            throw "Expected native gh shim at $ghShim, got $($resolvedGh.CommandType): $($resolvedGh.Source)"
        }
        $env:DOTFILES_SKIP_ATTESTATION = '0'
        $env:GH_PROMPT_DISABLED = 'caller'
        $env:DBUS_SESSION_BUS_ADDRESS = 'caller'
        $env:WRAPPER_TEST_GH_CAPTURE = $captureFile
        $env:WRAPPER_TEST_NATIVE_EXIT = '0'
        $stderr.GetStringBuilder().Clear() | Out-Null

        if (-not (Test-Attestation -Path 'fixture asset' -Repo 'fixture/releases')) {
            throw 'Native gh shim with stderr output and exit 0 was rejected under ErrorActionPreference=Stop'
        }
        if ($script:functionGhInvoked) {
            throw 'Test-Attestation invoked Function:gh instead of the native gh application'
        }
        if (-not (Test-Path -LiteralPath $captureFile)) {
            throw 'Native gh shim did not execute on exit 0 test'
        }
        $captured = @(Get-Content -LiteralPath $captureFile)
        $expectedCapture = @(
            'PROMPT=1'
            'DBUS=disabled:'
            'ARG1=attestation'
            'ARG2=verify'
            'ARG3=fixture asset'
            'ARG4=--repo'
            'ARG5=fixture/releases'
        )
        if (($captured -join '|') -cne ($expectedCapture -join '|')) {
            throw "Native gh received unexpected env/args: $($captured -join '|')"
        }
        Remove-Item -LiteralPath $captureFile -Force
        if ($ErrorActionPreference -cne 'Stop') {
            throw 'Test-Attestation did not restore ErrorActionPreference after native exit 0'
        }
        if ($env:GH_PROMPT_DISABLED -cne 'caller' -or $env:DBUS_SESSION_BUS_ADDRESS -cne 'caller') {
            throw 'Test-Attestation did not restore credential environment after native exit 0'
        }

        $env:WRAPPER_TEST_NATIVE_EXIT = '1'
        if (Test-Attestation -Path 'fixture asset' -Repo 'fixture/releases') {
            throw 'Native gh shim with stderr output and exit 1 accepted the asset'
        }
        if (-not (Test-Path -LiteralPath $captureFile)) {
            throw 'Native gh shim did not execute on exit 1 test'
        }
        if ($ErrorActionPreference -cne 'Stop') {
            throw 'Test-Attestation did not restore ErrorActionPreference after native exit 1'
        }
        if ($env:GH_PROMPT_DISABLED -cne 'caller' -or $env:DBUS_SESSION_BUS_ADDRESS -cne 'caller') {
            throw 'Test-Attestation did not restore credential environment after native exit 1'
        }
        if (-not $stderr.ToString().Contains('Check GitHub access and GH_TOKEN/GITHUB_TOKEN')) {
            throw 'Failed native gh verification did not emit token guidance on stderr'
        }

        # Native gh that fails to launch must fail closed even after a prior exit code 0.
        if ($isWindowsPlatform) {
            $brokenGh = Join-Path $brokenDir 'gh.exe'
            [System.IO.File]::WriteAllText($brokenGh, 'not a valid executable', $utf8NoBom)
        } else {
            $brokenGh = Join-Path $brokenDir 'gh'
            [System.IO.File]::WriteAllText($brokenGh, "#!/nonexistent/dotfiles-test-interpreter`n", $utf8NoBom)
            & chmod +x $brokenGh
            if ($LASTEXITCODE -ne 0) { throw "Failed to mark broken gh executable: $brokenGh" }
        }
        $env:PATH = $brokenDir
        $stderr.GetStringBuilder().Clear() | Out-Null
        $global:LASTEXITCODE = 0
        if (Test-Attestation -Path 'fixture asset' -Repo 'fixture/releases') {
            throw 'Unlaunchable native gh accepted the asset by inheriting stale LASTEXITCODE'
        }
        if ($ErrorActionPreference -cne 'Stop') {
            throw 'Test-Attestation did not restore ErrorActionPreference after launch failure'
        }
        if ($env:GH_PROMPT_DISABLED -cne 'caller' -or $env:DBUS_SESSION_BUS_ADDRESS -cne 'caller') {
            throw 'Test-Attestation did not restore credential environment after launch failure'
        }
        if (-not $stderr.ToString().Contains('WARNING: Build provenance verification failed')) {
            throw 'Launch failure did not emit verification failure warning on stderr'
        }
    } finally {
        [Console]::SetError($savedStderr)
        $stderr.Dispose()
        Remove-Item -LiteralPath 'Function:gh' -ErrorAction SilentlyContinue
        Remove-Variable functionGhInvoked -Scope Script -ErrorAction SilentlyContinue
        foreach ($name in $savedEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
        }
        Remove-Item -LiteralPath $nativeFixture -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-TestPass "Wrapper requires provenance unless explicitly bypassed"
    return $true
}

# ---------------------------------------------------------------------------
# Test Error Handling
# ---------------------------------------------------------------------------

function Test-ErrorHandling {
    Write-TestStage "Testing missing Cargo fails without executing or downloading a binary"
    $fixture = [System.IO.Path]::GetFullPath(
        (Join-Path (Join-Path $PSScriptRoot '../../../..') ".wrapper-no-cargo-$([guid]::NewGuid())"))
    $originalPath = $env:PATH
    $originalLocation = (Get-Location).Path
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        [void][System.IO.Directory]::CreateDirectory((Join-Path $fixture 'cli'))
        $wrapper = Join-Path $fixture 'dotfiles.ps1'
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../../../../dotfiles.ps1') -Destination $wrapper
        $env:PATH = ''
        $rejected = $false
        try {
            & $wrapper --build --version | Out-Null
        } catch {
            if ($_ -notmatch 'cargo not found') { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw 'Wrapper accepted an unavailable Cargo' }
        if (Test-Path -LiteralPath (Join-Path $fixture 'bin')) { throw 'Build failure attempted bootstrap' }
        if ((Get-Location).Path -ne $originalLocation) { throw 'Build failure changed the caller location' }
        Write-TestPass "Missing Cargo is rejected before any bootstrap or child execution"
        return $true
    } catch {
        Write-TestFail "Missing Cargo regression failed: $_"
        return $false
    } finally {
        $env:PATH = $originalPath
        foreach ($name in $savedEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
        }
        Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
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
    $backup = Join-Path $repo 'bin/.dotfiles.old'
    $buildBinary = Join-Path $repo "cli/target/dev-opt/$binaryName"
    $wrapper = Join-Path $repo 'dotfiles.ps1'
    $originalLocation = Get-Location
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER', 'DOTFILES_SKIP_ATTESTATION', 'DOTFILES_REEXEC_GUARD', 'DOTFILES_RELEASE_REPO',
            'WRAPPER_TEST_BUILD_EXIT', 'WRAPPER_TEST_BAD_CHECKSUM', 'WRAPPER_TEST_BAD_BINARY', 'WRAPPER_TEST_GH_EXIT',
            'WRAPPER_TEST_CONCURRENT_BINARY', 'GH_PROMPT_DISABLED', 'DBUS_SESSION_BUS_ADDRESS', 'PATH')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        [void][System.IO.Directory]::CreateDirectory((Split-Path $binary))
        [void][System.IO.Directory]::CreateDirectory((Split-Path $buildBinary))
        [void][System.IO.Directory]::CreateDirectory($caller)
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\dotfiles.ps1') -Destination $wrapper
        $nativeFixture = if ($IsLinux) {
            @'
#!/usr/bin/env python3
import json, os, sys
if sys.argv[1:] == ["--version"]:
    if os.environ.get("WRAPPER_TEST_BAD_BINARY") == "1":
        sys.exit(42)
    print("dotfiles v2026.09.17-1")
    sys.exit(0)
print(json.dumps({"Cwd": os.getcwd(), "Arguments": sys.argv[1:], "Root": os.environ["DOTFILES_ROOT"], "Wrapper": os.environ["DOTFILES_WRAPPER"], "Guard": os.environ["DOTFILES_REEXEC_GUARD"]}))
sys.exit(7)
'@
        } else { 'fixture' }
        [System.IO.File]::WriteAllText($binary, $nativeFixture)
        [System.IO.File]::WriteAllText($buildBinary, $nativeFixture)
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
        $smokeCheck = {
            if ($args.Count -ne 1 -or $args[0] -cne '--version') { throw 'Unexpected smoke check arguments' }
            $global:LASTEXITCODE = if ($env:WRAPPER_TEST_BAD_BINARY -eq '1') { 42 } else { 0 }
            'dotfiles v2026.09.17-1'
        }
        $registeredFunctions = [System.Collections.Generic.List[string]]::new()
        $binDir = Split-Path -Parent $binary
        if ($IsLinux) {
            & chmod +x $binary $buildBinary
            if ($LASTEXITCODE -ne 0) { throw 'Cannot make native fixture executable' }
        } else {
            Set-Item -LiteralPath "Function:global:$binary" -Value $child
            Set-Item -LiteralPath "Function:global:$buildBinary" -Value $child
            $registeredFunctions.Add($binary)
            $registeredFunctions.Add($buildBinary)
        }
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
        $ghBinDir = Join-Path $fixture 'gh-bin'
        [void][System.IO.Directory]::CreateDirectory($ghBinDir)
        $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
        if ($isWindowsPlatform) {
            $ghShimPath = Join-Path $ghBinDir 'gh.cmd'
            $ghShimScript = @(
                '@echo off'
                'if not "%GH_PROMPT_DISABLED%"=="1" exit /b 99'
                'if not "%DBUS_SESSION_BUS_ADDRESS%"=="disabled:" exit /b 99'
                'if "%WRAPPER_TEST_GH_EXIT%"=="publish" ('
                '  <nul set /p =concurrent fixture>"%WRAPPER_TEST_CONCURRENT_BINARY%"'
                '  exit /b 0'
                ')'
                'if "%WRAPPER_TEST_GH_EXIT%"=="throw" ('
                '  <nul set /p =concurrent fixture>"%WRAPPER_TEST_CONCURRENT_BINARY%"'
                '  exit /b 1'
                ')'
                'exit /b %WRAPPER_TEST_GH_EXIT%'
            ) -join "`r`n"
            [System.IO.File]::WriteAllText($ghShimPath, "$ghShimScript`r`n", $utf8NoBom)
        } else {
            $ghShimPath = Join-Path $ghBinDir 'gh'
            $ghShimScript = @(
                '#!/bin/sh'
                'if [ "${GH_PROMPT_DISABLED:-}" != "1" ] || [ "${DBUS_SESSION_BUS_ADDRESS:-}" != "disabled:" ]; then exit 99; fi'
                'if [ "${WRAPPER_TEST_GH_EXIT:-0}" = "publish" ]; then'
                '  printf "concurrent fixture" > "$WRAPPER_TEST_CONCURRENT_BINARY"'
                '  exit 0'
                'fi'
                'if [ "${WRAPPER_TEST_GH_EXIT:-0}" = "throw" ]; then'
                '  printf "concurrent fixture" > "$WRAPPER_TEST_CONCURRENT_BINARY"'
                '  exit 1'
                'fi'
                'exit "${WRAPPER_TEST_GH_EXIT:-0}"'
            ) -join "`n"
            [System.IO.File]::WriteAllText($ghShimPath, "$ghShimScript`n", $utf8NoBom)
            & chmod +x $ghShimPath
            if ($LASTEXITCODE -ne 0) { throw "Failed to mark native gh shim executable: $ghShimPath" }
        }
        $env:PATH = "$ghBinDir$([System.IO.Path]::PathSeparator)$($savedEnvironment['PATH'])"
        $env:WRAPPER_TEST_CONCURRENT_BINARY = $binary
        $downloadCapture = @{ Count = 0; LastPath = $null; LastDir = $null }
        Set-Item Function:Invoke-WebRequest -Value {
            param([string]$Uri, [string]$OutFile)
            if ($Uri.EndsWith('/releases/latest')) {
                return @{ Content = '{"tag_name":"v2026.09.17-1"}' }
            }
            if ($Uri.EndsWith('/checksums.sha256')) {
                $hash = (Get-FileHash -LiteralPath $downloadCapture.LastPath -Algorithm SHA256).Hash
                if ($env:WRAPPER_TEST_BAD_CHECKSUM -eq '1') { $hash = '0' * 64 }
                return @{ Content = "$hash  $(Get-TargetAssetName)" }
            }
            if ($OutFile) {
                $stageDir = Split-Path -Parent $OutFile
                if ((Split-Path -Leaf $stageDir) -cnotmatch '^\.bootstrap-[0-9a-f]{32}$') {
                    throw "Unexpected bootstrap staging directory: $OutFile"
                }
                $downloadCapture.Count++
                $downloadCapture.LastPath = $OutFile
                $downloadCapture.LastDir = $stageDir
                [System.IO.File]::WriteAllText($OutFile, $nativeFixture)
                if (-not $IsLinux) {
                    # Register in global scope so the mock survives Invoke-WebRequest returning.
                    Set-Item -LiteralPath "Function:global:$OutFile" -Value $smokeCheck
                    $registeredFunctions.Add($OutFile)
                }
                return
            }
            throw "Unexpected fixture request: $Uri"
        }
        $env:DOTFILES_SKIP_ATTESTATION = '0'
        $env:DOTFILES_REEXEC_GUARD = '1'
        $env:DOTFILES_RELEASE_REPO = 'fixture/releases'
        $env:WRAPPER_TEST_BUILD_EXIT = '0'
        $env:WRAPPER_TEST_BAD_CHECKSUM = '0'
        $env:WRAPPER_TEST_BAD_BINARY = '0'
        $env:WRAPPER_TEST_GH_EXIT = '0'
        $env:GH_PROMPT_DISABLED = 'caller'
        $env:DBUS_SESSION_BUS_ADDRESS = 'caller'
        Set-Location -LiteralPath $caller
        if (Test-Path Variable:PSNativeCommandArgumentPassing) {
            Set-Variable -Name PSNativeCommandArgumentPassing -Value Legacy -Scope Local
        }
        $arguments = @('update', '--dry-run', '--root', '.', '--overlay', '../overlay "dir"', '', '--BUILD', '--', 'literal value', '--build', '')
        $modes = @('cached', 'build', 'bootstrap', 'empty-cache')
        if ($IsLinux) { $modes += @('non-executable', 'legacy-backup') }
        foreach ($mode in $modes) {
            $beforeDownloads = $downloadCapture.Count
            $wrapperArguments = $arguments
            if ($mode -eq 'build') { $wrapperArguments = @('--build') + $arguments }
            if ($mode -eq 'bootstrap') { Remove-Item -LiteralPath $binary }
            if ($mode -eq 'empty-cache') { [IO.File]::WriteAllText($binary, '') }
            if ($mode -eq 'non-executable') { & chmod a-x $binary }
            if ($mode -eq 'legacy-backup') { [IO.File]::Move($binary, $backup) }
            $output = @(& $wrapper @wrapperArguments)
            if ($env:GH_PROMPT_DISABLED -cne 'caller' -or $env:DBUS_SESSION_BUS_ADDRESS -cne 'caller') {
                throw "$mode changed the caller's credential environment"
            }
            if ($LASTEXITCODE -ne 7) { throw "$mode lost the child exit code" }
            if ($output.Count -ne 1) { throw "$mode mixed wrapper messages into child stdout" }
            $actual = $output[0] | ConvertFrom-Json
            if ($actual.Cwd -ne $caller) { throw "$mode changed the child working directory" }
            if ($actual.Root -ne $repo -or $actual.Wrapper -cne 'pwsh' -or $actual.Guard -cne '1') {
                throw "$mode changed runtime context"
            }
            if (($actual.Arguments | ConvertTo-Json -Compress) -cne ($arguments | ConvertTo-Json -Compress)) {
                throw "$mode changed the forwarded arguments"
            }
            if ($mode -eq 'legacy-backup') {
                if ((Test-Path -LiteralPath $binary) -or -not (Test-Path -LiteralPath $backup -PathType Leaf)) {
                    throw 'Legacy recovery moved or removed the backup image'
                }
                [IO.File]::Move($backup, $binary)
            } elseif (-not (Test-Path -LiteralPath $binary)) {
                throw "$mode did not preserve the cached binary"
            }
            $expectedDownloads = $beforeDownloads + [int]($mode -in @('bootstrap', 'empty-cache', 'non-executable'))
            if ($downloadCapture.Count -ne $expectedDownloads) { throw "$mode used the wrong bootstrap path" }
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
        $env:WRAPPER_TEST_GH_EXIT = '4'
        $rejected = $false
        try {
            & $wrapper --version | Out-Null
        } catch {
            if ($_ -notmatch 'Build provenance verification failed') { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw 'Wrapper accepted an unauthenticated attestation' }
        if ($env:GH_PROMPT_DISABLED -cne 'caller' -or $env:DBUS_SESSION_BUS_ADDRESS -cne 'caller') {
            throw 'Failed verification changed the caller credential environment'
        }
        if ((Test-Path -LiteralPath $binary) -or (Test-Path -LiteralPath $downloadCapture.LastDir)) {
            throw 'Failed verification left a downloaded binary'
        }
        [System.IO.File]::WriteAllText($binary, 'fixture')
        $env:WRAPPER_TEST_GH_EXIT = '0'

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
        if ((Test-Path -LiteralPath $binary) -or (Test-Path -LiteralPath $downloadCapture.LastDir)) {
            throw 'Rejected download was not removed from the literal path'
        }
        $env:WRAPPER_TEST_BAD_CHECKSUM = '0'
        $env:WRAPPER_TEST_BAD_BINARY = '1'
        $rejected = $false
        try {
            & $wrapper --version | Out-Null
        } catch {
            if ($_ -notmatch 'Downloaded CLI did not start') { throw }
            $rejected = $true
        }
        if (-not $rejected -or (Test-Path -LiteralPath $binary) -or (Test-Path -LiteralPath $downloadCapture.LastDir)) {
            throw 'Bootstrap published or leaked a verified binary that did not start'
        }
        $env:WRAPPER_TEST_BAD_BINARY = '0'
        foreach ($repair in @($false, $true)) {
            if ($repair) {
                [IO.File]::WriteAllText($binary, '')
                if ($IsLinux) { & chmod +x $binary }
            }
            $env:WRAPPER_TEST_GH_EXIT = 'publish'
            $rejected = $false
            try {
                & $wrapper --version | Out-Null
            } catch {
                if ($_ -notmatch 'Failed to install downloaded binary') { throw }
                $rejected = $true
            }
            if (-not $rejected -or [System.IO.File]::ReadAllText($binary) -cne 'concurrent fixture' -or
                (Test-Path -LiteralPath $downloadCapture.LastDir)) { throw 'Bootstrap overwrote a concurrently installed CLI' }
            Remove-Item -LiteralPath $binary
        }
        $env:WRAPPER_TEST_GH_EXIT = 'throw'
        $rejected = $false
        try {
            & $wrapper --version | Out-Null
        } catch {
            if ($_ -notmatch 'Build provenance verification failed') { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw 'Wrapper ignored an attestation failure during concurrent install' }
        if (Test-Path -LiteralPath $downloadCapture.LastDir) {
            throw 'Unexpected verification failure leaked the downloaded binary'
        }
        if (@(Get-ChildItem -LiteralPath $binDir -Force -Filter '.bootstrap-*').Count -ne 0) {
            throw 'Bootstrap left a staging directory behind'
        }
        if ([System.IO.File]::ReadAllText($binary) -cne 'concurrent fixture') {
            throw 'Failed bootstrap changed the concurrently installed binary'
        }
        if ($env:GH_PROMPT_DISABLED -cne 'caller' -or $env:DBUS_SESSION_BUS_ADDRESS -cne 'caller') {
            throw 'Attestation failure changed the caller credential environment'
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
        # PowerShell's Function provider ignores a global: prefix on Remove-Item,
        # whereas Function:<name> resolves and removes the global function cleanly.
        foreach ($fn in $registeredFunctions) {
            Remove-Item -LiteralPath "Function:$fn" -ErrorAction SilentlyContinue
        }
        Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-NativeExitCode {
    Write-TestStage 'Testing native arguments and exit codes with caller preferences'
    $fixture = Join-Path (Join-Path $PSScriptRoot '..\..\..\..') ".wrapper-native-exit-$([guid]::NewGuid())"
    $fixture = [System.IO.Path]::GetFullPath($fixture)
    $wrapper = Join-Path $fixture 'dotfiles.ps1'
    $nativeShell = (Get-Command pwsh -CommandType Application | Select-Object -First 1).Source
    $originalLocation = Get-Location
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        [void][System.IO.Directory]::CreateDirectory((Join-Path $fixture 'cli'))
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot '..\..\..\..\dotfiles.ps1') -Destination $wrapper
        $PSNativeCommandUseErrorActionPreference = $true
        Set-Item Function:cargo -Value {
            @{
                reason = 'compiler-artifact'
                target = @{ name = 'dotfiles' }
                executable = $nativeShell
            } | ConvertTo-Json -Compress
            $global:LASTEXITCODE = 0
        }
        $stdout = Join-Path $fixture 'stdout'
        $stderr = Join-Path $fixture 'stderr'
        & $wrapper --build -NoProfile -Command '[Console]::Out.WriteLine("child stdout"); [Console]::Error.WriteLine("child stderr"); exit 7' `
            > $stdout 2> $stderr
        if ($LASTEXITCODE -ne 7) { throw 'Wrapper lost the native child exit code' }
        if ((Get-Content -LiteralPath $stdout -Raw).Trim() -cne 'child stdout') {
            throw 'Wrapper changed native child stdout'
        }
        if ((Get-Content -LiteralPath $stderr -Raw).Trim() -cne 'child stderr') {
            throw 'Wrapper changed native child stderr'
        }
        if (-not $PSNativeCommandUseErrorActionPreference) { throw 'Wrapper changed the caller native error preference' }

        if (Test-Path Variable:PSNativeCommandArgumentPassing) {
            $originalArgumentPreference = $PSNativeCommandArgumentPassing
            $PSNativeCommandArgumentPassing = 'Legacy'
            $probe = Join-Path $fixture 'native argv.ps1'
            [IO.File]::WriteAllText($probe, 'ConvertTo-Json -InputObject @($args) -Compress')
            $directory = (Join-Path $fixture 'path with spaces') + [IO.Path]::DirectorySeparatorChar
            $expected = @('quoted "value"', '', $directory)
            $output = @(& $wrapper --build -NoProfile -File $probe @expected)
            if ($LASTEXITCODE -ne 0 -or $output.Count -ne 1 -or
                $output[0] -cne (ConvertTo-Json -InputObject $expected -Compress)) {
                throw 'Wrapper changed native child arguments under the caller legacy preference'
            }
            if ($PSNativeCommandArgumentPassing -cne 'Legacy') {
                throw 'Wrapper changed the caller native argument preference'
            }
            $PSNativeCommandArgumentPassing = $originalArgumentPreference
        }

        Set-Item Function:cargo -Value {
            & $nativeShell -NoProfile -Command '[Console]::Error.WriteLine("cargo failed"); exit 23'
        }
        & $wrapper --build --version > $stdout 2> $stderr
        if ($LASTEXITCODE -ne 23) { throw 'Wrapper lost the native Cargo exit code' }
        if ((Get-Content -LiteralPath $stdout -Raw)) { throw 'Wrapper ran a child after Cargo failed' }
        if ((Get-Content -LiteralPath $stderr -Raw).Trim() -cne 'cargo failed') {
            throw 'Wrapper changed native Cargo stderr'
        }
        if ((Get-Location).Path -ne $originalLocation.Path) { throw 'Failed native Cargo changed the caller location' }
        if (-not $PSNativeCommandUseErrorActionPreference) { throw 'Failed build changed the caller native error preference' }
        Write-TestPass 'Native child arguments and exit codes are preserved independently of caller preferences'
        return $true
    } catch {
        Write-TestFail "Native exit-code regression failed: $_"
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
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER', 'CARGO_TARGET_DIR', 'PATH')) {
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
            '{"reason":"compiler-artifact","target":{"name":"another-binary"},"executable":"not-the-dotfiles-artifact"}'
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
            if ($IsLinux) {
                [System.IO.File]::WriteAllText($artifact, @'
#!/usr/bin/env python3
import json, os, sys
print(json.dumps({"Cwd": os.getcwd(), "Arguments": sys.argv[1:]}))
sys.exit(7)
'@)
                & chmod +x $artifact
            } else {
                [System.IO.File]::WriteAllText($artifact, 'new')
                Set-Item -LiteralPath "Function:$artifact" -Value $child
            }
            $arguments = @('--version', 'space value', '')
            $output = @(& $wrapper --build @arguments)
            if ($LASTEXITCODE -ne 7 -or $output.Count -ne 1) { throw "$mode did not run only the reported artifact" }
            $actual = $output[0] | ConvertFrom-Json
            if ($actual.Cwd -ne $caller) { throw "$mode changed the caller cwd" }
            if (($actual.Arguments | ConvertTo-Json -Compress) -cne ($arguments | ConvertTo-Json -Compress)) {
                throw "$mode changed child arguments"
            }
        }
        if ($IsLinux) {
            # Never launch a real desktop application, even if the guard regresses.
            $opener = Join-Path $fixture 'xdg-open'
            [IO.File]::WriteAllText($opener, "#!/bin/sh`nexit 0`n")
            & chmod +x $opener
            $env:PATH = "${fixture}:$env:PATH"
            & chmod a-x $artifact
            $rejected = $false
            try {
                & $wrapper --build --version | Out-Null
            } catch {
                if ($_ -notmatch 'not an executable file') { throw }
                $rejected = $true
            }
            if (-not $rejected) { throw 'Build mode accepted a non-executable artifact' }
        }
        Write-TestPass 'Build mode executes only the reported Cargo artifact from the caller directory'
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

function Test-CargoArtifactFailure {
    Write-TestStage 'Testing missing and ambiguous Cargo artifacts fail before child execution'
    $fixture = [System.IO.Path]::GetFullPath(
        (Join-Path (Join-Path $PSScriptRoot '../../../..') ".wrapper-artifact-errors-$([guid]::NewGuid())"))
    $originalLocation = (Get-Location).Path
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        [void][System.IO.Directory]::CreateDirectory((Join-Path $fixture 'cli'))
        $wrapper = Join-Path $fixture 'dotfiles.ps1'
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../../../../dotfiles.ps1') -Destination $wrapper
        foreach ($failure in @('missing', 'wrong-target', 'duplicate')) {
            Set-Item Function:cargo -Value {
                if ($failure -eq 'wrong-target') {
                    '{"reason":"compiler-artifact","target":{"name":"another-binary"},"executable":"must-not-run"}'
                } elseif ($failure -eq 'duplicate') {
                    foreach ($unused in 1..2) {
                        @{
                            reason = 'compiler-artifact'
                            target = @{ name = 'dotfiles' }
                            executable = Join-Path $fixture 'must-not-run'
                        } | ConvertTo-Json -Compress
                    }
                }
                '{"reason":"build-finished","success":true}'
                $global:LASTEXITCODE = 0
            }
            $expectedError = if ($failure -eq 'duplicate') { 'Cargo reported more than one' } else { 'Cargo did not report' }
            $rejected = $false
            try {
                & $wrapper --build --version | Out-Null
            } catch {
                if ($_ -notmatch $expectedError) { throw }
                $rejected = $true
            }
            if (-not $rejected) { throw "$failure Cargo artifact did not fail" }
            if ((Get-Location).Path -ne $originalLocation) { throw "$failure artifact changed the caller location" }
        }
        Write-TestPass 'Missing, wrong-target, and ambiguous artifacts cannot execute a child or retain cli/ as cwd'
        return $true
    } catch {
        Write-TestFail "Cargo artifact rejection failed: $_"
        return $false
    } finally {
        Set-Location -LiteralPath $originalLocation
        foreach ($name in $savedEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
        }
        Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# ---------------------------------------------------------------------------
# Run All Tests
# ---------------------------------------------------------------------------

function Test-ReleaseRepository {
    Write-TestStage 'Testing GitHub release repository discovery and overrides'
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_RELEASE_REPO', 'DOTFILES_ROOT', 'DOTFILES_WRAPPER', 'PATH', 'WRAPPER_TEST_NATIVE_EXIT', 'WRAPPER_TEST_NATIVE_MARKER', 'WRAPPER_TEST_NATIVE_STDOUT')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    $fixture = [System.IO.Path]::GetFullPath(
        (Join-Path (Join-Path $PSScriptRoot '../../../..') ".wrapper-release-repo-$([guid]::NewGuid())"))
    try {
        $wrapper = Join-Path $PSScriptRoot '../../../../dotfiles.ps1'
        $tokens = $null
        $errors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile(
            $wrapper, [ref]$tokens, [ref]$errors)
        if ($errors.Count -ne 0) { throw "Wrapper parse failed: $errors" }
        $resolver = $ast.Find({ param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq 'Resolve-ReleaseRepo'
        }, $true)
        . ([scriptblock]::Create($resolver.Extent.Text))
        # The resolver loaded above reads this variable through dynamic scope.
        Set-Variable -Name DotfilesRoot -Value 'fixture root with spaces'

        # Exercise a real native git child process writing to stderr on exit 0
        # and exit 1 before installing PowerShell function mocks.
        $shimDir = Join-Path $fixture 'bin'
        [void][System.IO.Directory]::CreateDirectory($shimDir)
        $gitShim = Write-NativeStderrShim -Directory $shimDir -Name 'git'
        $marker = Join-Path $fixture 'git-invoked'
        $env:PATH = "$shimDir$([System.IO.Path]::PathSeparator)$($savedEnvironment['PATH'])"
        $resolvedGit = Get-Command git -ErrorAction Stop
        if ($resolvedGit.CommandType -ne 'Application' -or $resolvedGit.Source -ne $gitShim) {
            throw "Expected native git shim at $gitShim, got $($resolvedGit.CommandType): $($resolvedGit.Source)"
        }
        Remove-Item Env:DOTFILES_RELEASE_REPO -ErrorAction SilentlyContinue
        $env:WRAPPER_TEST_NATIVE_MARKER = $marker
        $env:WRAPPER_TEST_NATIVE_STDOUT = 'https://github.com/fork-owner/custom-name.git'
        $env:WRAPPER_TEST_NATIVE_EXIT = '0'
        if ((Resolve-ReleaseRepo) -cne 'fork-owner/custom-name') {
            throw 'Native git shim with stderr output and exit 0 discarded a valid GitHub remote under ErrorActionPreference=Stop'
        }
        if (-not (Test-Path -LiteralPath $marker)) {
            throw 'Native git shim did not execute on exit 0 test'
        }
        Remove-Item -LiteralPath $marker -Force
        if ($ErrorActionPreference -cne 'Stop') {
            throw 'Resolve-ReleaseRepo did not restore ErrorActionPreference after native exit 0'
        }
        $env:WRAPPER_TEST_NATIVE_STDOUT = ''
        $env:WRAPPER_TEST_NATIVE_EXIT = '1'
        $rejected = $false
        try { Resolve-ReleaseRepo | Out-Null } catch {
            if ($_ -notmatch 'DOTFILES_RELEASE_REPO') { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw 'Failed native git lookup with stderr accepted' }
        if (-not (Test-Path -LiteralPath $marker)) {
            throw 'Native git shim did not execute on exit 1 test'
        }
        if ($ErrorActionPreference -cne 'Stop') {
            throw 'Resolve-ReleaseRepo did not restore ErrorActionPreference after native exit 1'
        }
        $env:PATH = $savedEnvironment['PATH']

        $script:releaseRemote = $null
        $script:releaseGitStderr = $null
        function git {
            if (($args -join '|') -cne '-C|fixture root with spaces|remote|get-url|origin') {
                throw 'Repository discovery used the wrong arguments'
            }
            if ($script:releaseGitStderr) { Write-Error $script:releaseGitStderr }
            $global:LASTEXITCODE = if ($null -eq $script:releaseRemote) { 1 } else { 0 }
            $script:releaseRemote
        }
        Remove-Item Env:DOTFILES_RELEASE_REPO -ErrorAction SilentlyContinue
        foreach ($remote in @('https://github.com/fork-owner/custom-name.git',
                'git@github.com:fork-owner/custom-name.git',
                'ssh://git@github.com/fork-owner/custom-name.git')) {
            $script:releaseRemote = $remote
            if ((Resolve-ReleaseRepo) -cne 'fork-owner/custom-name') { throw "Wrong repository: $remote" }
        }
        $script:releaseGitStderr = 'git stderr warning'
        $script:releaseRemote = 'https://github.com/fork-owner/custom-name.git'
        if ((Resolve-ReleaseRepo) -cne 'fork-owner/custom-name') {
            throw 'Stderr warning discarded a valid GitHub remote under ErrorActionPreference=Stop'
        }
        if ($ErrorActionPreference -cne 'Stop') {
            throw 'Resolve-ReleaseRepo did not restore ErrorActionPreference after stderr warning'
        }
        $script:releaseRemote = $null
        $rejected = $false
        try { Resolve-ReleaseRepo | Out-Null } catch {
            if ($_ -notmatch 'DOTFILES_RELEASE_REPO') { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw 'Failed git lookup with stderr accepted' }
        if ($ErrorActionPreference -cne 'Stop') {
            throw 'Resolve-ReleaseRepo did not restore ErrorActionPreference after failed lookup'
        }
        $script:releaseGitStderr = $null
        $script:releaseRemote = 'https://example.com/other/repo'
        $env:DOTFILES_RELEASE_REPO = 'publisher/releases'
        if ((Resolve-ReleaseRepo) -cne 'publisher/releases') { throw 'Override was ignored' }
        foreach ($override in @('owner/repo/extra', '../repo', 'owner/repo?query')) {
            $env:DOTFILES_RELEASE_REPO = $override
            $rejected = $false
            try { Resolve-ReleaseRepo | Out-Null } catch {
                if ($_ -notmatch 'DOTFILES_RELEASE_REPO') { throw }
                $rejected = $true
            }
            if (-not $rejected) { throw "Invalid override accepted: $override" }
        }
        Remove-Item Env:DOTFILES_RELEASE_REPO
        foreach ($remote in @('https://example.com/other/repo', $null)) {
            $script:releaseRemote = $remote
            $rejected = $false
            try { Resolve-ReleaseRepo | Out-Null } catch {
                if ($_ -notmatch 'DOTFILES_RELEASE_REPO') { throw }
                $rejected = $true
            }
            if (-not $rejected) { throw 'Unsupported or missing remote accepted' }
        }
        # Exercise the actual wrapper's bootstrap wiring with a network fixture.
        [void][System.IO.Directory]::CreateDirectory($fixture)
        $fixtureWrapper = Join-Path $fixture 'dotfiles.ps1'
        Copy-Item -LiteralPath $wrapper -Destination $fixtureWrapper
        Set-Item Function:Invoke-WebRequest -Value {
            param($Uri, [switch]$UseBasicParsing, $TimeoutSec)
            if (-not $UseBasicParsing -or $TimeoutSec -ne 120) {
                throw 'Bootstrap changed its metadata request options'
            }
            $releaseCapture.Uri = $Uri
            return @{ Content = '{}' }
        }
        $env:DOTFILES_RELEASE_REPO = 'publisher/releases'
        $releaseCapture = @{ Uri = $null }
        try { & $fixtureWrapper --version | Out-Null } catch {
            if ($_ -notmatch 'Failed to resolve latest release tag') { throw }
        }
        if ($releaseCapture.Uri -cne 'https://api.github.com/repos/publisher/releases/releases/latest') {
            throw 'Bootstrap did not pass the selected publisher to release lookup'
        }
        Write-TestPass 'Release repository discovery, override, and rejection cases'
        return $true
    } catch {
        Write-TestFail "Release repository regression failed: $_"
        return $false
    } finally {
        Remove-Item -LiteralPath 'Function:Invoke-WebRequest' -ErrorAction SilentlyContinue
        foreach ($name in $savedEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
        }
        Remove-Variable releaseRemote -Scope Script -ErrorAction SilentlyContinue
        Remove-Variable releaseGitStderr -Scope Script -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-CliStartupCompletionEnv {
    Write-TestStage 'Testing CLI startup probe clears DOTFILES_COMPLETE while preserving it for dispatch'
    $isWindowsPlatform = ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
    $binaryName = if ($isWindowsPlatform) { 'dotfiles.exe' } else { 'dotfiles' }
    $backupName = if ($isWindowsPlatform) { '.dotfiles-old.exe' } else { '.dotfiles.old' }
    $fixture = [System.IO.Path]::GetFullPath(
        (Join-Path (Join-Path $PSScriptRoot '../../../..') ".wrapper-complete-probe-$([guid]::NewGuid())"))
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_COMPLETE', 'DOTFILES_ROOT', 'DOTFILES_WRAPPER')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        $wrapperSource = Join-Path $PSScriptRoot '../../../../dotfiles.ps1'
        $tokens = $null
        $errors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile(
            $wrapperSource, [ref]$tokens, [ref]$errors)
        if ($errors.Count -ne 0) { throw "Wrapper parse failed: $errors" }
        $probeAst = $ast.Find({ param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -eq 'Test-CliStartup'
        }, $true)
        if (-not $probeAst) { throw 'Wrapper has no Test-CliStartup function' }
        . ([scriptblock]::Create($probeAst.Extent.Text))

        $binDir = Join-Path $fixture 'bin'
        [void][System.IO.Directory]::CreateDirectory($binDir)
        $wrapper = Join-Path $fixture 'dotfiles.ps1'
        Copy-Item -LiteralPath $wrapperSource -Destination $wrapper
        $binary = Join-Path $binDir $binaryName
        $backup = Join-Path $binDir $backupName
        $journal = Join-Path $binDir ".$binaryName.update-state"

        if ($env:BINARY_PATH -and (Test-Path -LiteralPath $env:BINARY_PATH -PathType Leaf)) {
            Copy-Item -LiteralPath $env:BINARY_PATH -Destination $binary
            Copy-Item -LiteralPath $env:BINARY_PATH -Destination $backup
        } elseif (-not $isWindowsPlatform) {
            $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
            $fakeCli = @(
                '#!/bin/sh'
                'if [ -n "${DOTFILES_COMPLETE:-}" ]; then'
                '  printf "Register-ArgumentCompleter -Native -CommandName dotfiles (%s)\n" "$DOTFILES_COMPLETE"'
                '  exit 0'
                'fi'
                'if [ "${1:-}" = "--version" ]; then'
                '  printf "dotfiles v2026.09.17-1\n"'
                '  exit 0'
                'fi'
                'exit 1'
            ) -join "`n"
            [System.IO.File]::WriteAllText($binary, "$fakeCli`n", $utf8NoBom)
            [System.IO.File]::WriteAllText($backup, "$fakeCli`n", $utf8NoBom)
            & chmod +x $binary $backup
            if ($LASTEXITCODE -ne 0) { throw 'Failed to mark completion probe fixture executable' }
        } else {
            Write-Information 'Skipping native completion probe on Windows without BINARY_PATH' -InformationAction Continue
            return $true
        }

        $env:DOTFILES_COMPLETE = 'powershell'
        if (-not (Test-CliStartup $binary)) {
            throw 'Test-CliStartup rejected a healthy CLI when DOTFILES_COMPLETE was set'
        }
        if ($env:DOTFILES_COMPLETE -cne 'powershell') {
            throw 'Test-CliStartup mutated DOTFILES_COMPLETE in the caller environment'
        }

        # With an update journal present and DOTFILES_COMPLETE set, recovery must
        # keep the healthy installed CLI, clear the journal, and still pass
        # DOTFILES_COMPLETE through to the dispatched CLI.
        [System.IO.File]::WriteAllText($journal, 'pending')
        $output = @(& $wrapper)
        if ($LASTEXITCODE -ne 0) {
            throw "Wrapper failed during completion registration with update journal (exit $LASTEXITCODE)"
        }
        $joined = ($output -join "`n")
        if ($joined -notmatch 'Register-ArgumentCompleter') {
            throw "Dispatched CLI did not receive DOTFILES_COMPLETE=powershell: $joined"
        }
        if ((Get-Item -Force -LiteralPath $journal).Length -ne 0) {
            throw 'Healthy CLI recovery did not clear the update journal'
        }
        if (-not (Test-Path -LiteralPath $backup -PathType Leaf)) {
            throw 'Healthy CLI recovery unexpectedly overwrote the installed CLI from backup'
        }
        if ($env:DOTFILES_COMPLETE -cne 'powershell') {
            throw 'Wrapper mutated DOTFILES_COMPLETE in the caller environment'
        }

        Write-TestPass 'CLI startup probe clears DOTFILES_COMPLETE without stripping it from dispatch'
        return $true
    } catch {
        Write-TestFail "CLI startup completion-env regression failed: $_"
        return $false
    } finally {
        foreach ($name in $savedEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
        }
        Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Invoke-TestSuite {
    $results = @()

    $results += Test-BuildMode
    $results += Test-VersionDetection
    $results += Test-ChecksumVerification
    $results += Test-ArgumentForwarding
    $results += Test-UpdateArgumentForwarding
    $results += Test-AdvancedFlagForwarding
    $results += Test-VersionPinnedBootstrapUrl
    $results += Test-AttestationVerification
    $results += Test-CliStartupCompletionEnv
    $results += Test-ErrorHandling
    $results += Test-IsolatedWrapperPath
    $results += Test-NativeExitCode
    $results += Test-CargoArtifactPath
    $results += Test-CargoArtifactFailure
    $results += Test-ReleaseRepository

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
