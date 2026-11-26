# Wrapper error handling, isolation, and native exit code tests.

function Test-ErrorHandling {
    Write-TestStage "Testing missing Cargo fails without executing or downloading a binary"
    $fixture = [System.IO.Path]::GetFullPath(
        (Join-Path (Get-RepoRoot) ".wrapper-no-cargo-$([guid]::NewGuid())"))
    $originalPath = $env:PATH
    $originalLocation = (Get-Location).Path
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        [void][System.IO.Directory]::CreateDirectory((Join-Path $fixture 'cli'))
        $wrapper = Join-Path $fixture 'dotfiles.ps1'
        Copy-Item -LiteralPath (Get-WrapperPath) -Destination $wrapper
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
    $fixture = Join-Path (Get-RepoRoot) ".wrapper-context-$([guid]::NewGuid())"
    $fixture = [System.IO.Path]::GetFullPath($fixture)
    $repo = Join-Path $fixture 'repo [literal]'
    $caller = Join-Path $fixture 'caller directory'
    $isWindowsPlatform = ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
    $binaryName = if ($isWindowsPlatform) { 'dotfiles.exe' } else { 'dotfiles' }
    $backupName = if ($isWindowsPlatform) { '.dotfiles-old.exe' } else { '.dotfiles.old' }
    $binary = Join-Path $repo "bin/$binaryName"
    $backup = Join-Path $repo "bin/$backupName"
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
        Copy-Item -LiteralPath (Get-WrapperPath) -Destination $wrapper
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
    $fixture = Join-Path (Get-RepoRoot) ".wrapper-native-exit-$([guid]::NewGuid())"
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
        Copy-Item -LiteralPath (Get-WrapperPath) -Destination $wrapper
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
