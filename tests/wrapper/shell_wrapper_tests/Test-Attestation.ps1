# Attestation verification and release repository tests.

function Test-AttestationVerification {
    Write-TestStage "Testing build provenance verification in bootstrap download"

    $wrapper = Get-WrapperPath
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
        (Join-Path (Get-RepoRoot) ".wrapper-native-gh-$([guid]::NewGuid())"))
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

function Test-ReleaseRepository {
    Write-TestStage 'Testing GitHub release repository discovery and overrides'
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_RELEASE_REPO', 'DOTFILES_ROOT', 'DOTFILES_WRAPPER', 'PATH', 'WRAPPER_TEST_NATIVE_EXIT', 'WRAPPER_TEST_NATIVE_MARKER', 'WRAPPER_TEST_NATIVE_STDOUT')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    $fixture = [System.IO.Path]::GetFullPath(
        (Join-Path (Get-RepoRoot) ".wrapper-release-repo-$([guid]::NewGuid())"))
    try {
        $wrapper = Get-WrapperPath
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
