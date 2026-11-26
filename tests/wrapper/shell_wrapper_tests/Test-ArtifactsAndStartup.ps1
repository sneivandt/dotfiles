# Cargo artifact selection and CLI startup completion env tests.

function Test-CargoArtifactPath {
    Write-TestStage 'Testing Cargo output directories and stale-artifact rejection'
    $fixture = Join-Path (Get-RepoRoot) ".wrapper-artifact-$([guid]::NewGuid())"
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
        Copy-Item -LiteralPath (Get-WrapperPath) -Destination $wrapper
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
        (Join-Path (Get-RepoRoot) ".wrapper-artifact-errors-$([guid]::NewGuid())"))
    $originalLocation = (Get-Location).Path
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        [void][System.IO.Directory]::CreateDirectory((Join-Path $fixture 'cli'))
        $wrapper = Join-Path $fixture 'dotfiles.ps1'
        Copy-Item -LiteralPath (Get-WrapperPath) -Destination $wrapper
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

function Test-CliStartupCompletionEnv {
    Write-TestStage 'Testing CLI startup probe clears DOTFILES_COMPLETE while preserving it for dispatch'
    $isWindowsPlatform = ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
    $binaryName = if ($isWindowsPlatform) { 'dotfiles.exe' } else { 'dotfiles' }
    $backupName = if ($isWindowsPlatform) { '.dotfiles-old.exe' } else { '.dotfiles.old' }
    $fixture = [System.IO.Path]::GetFullPath(
        (Join-Path (Get-RepoRoot) ".wrapper-complete-probe-$([guid]::NewGuid())"))
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_COMPLETE', 'DOTFILES_ROOT', 'DOTFILES_WRAPPER')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        $wrapperSource = Get-WrapperPath
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
