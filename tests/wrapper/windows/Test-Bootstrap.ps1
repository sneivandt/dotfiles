# Bootstrap, version detection, checksum, and argument forwarding tests.

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
        $output = & (Get-WrapperPath) --build --version 2>&1
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

function Test-VersionDetection {
    Write-TestStage "Testing version detection through wrapper"

    if (-not $env:BINARY_PATH -or -not (Test-Path -LiteralPath $env:BINARY_PATH -PathType Leaf)) {
        Write-Information "Skipping: BINARY_PATH not set or binary not found" -InformationAction Continue
        return $true
    }

    $isWindowsPlatform = ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
    $binaryName = if ($isWindowsPlatform) { 'dotfiles.exe' } else { 'dotfiles' }
    $fixture = [System.IO.Path]::GetFullPath(
        (Join-Path (Get-RepoRoot) ".wrapper-version-$([guid]::NewGuid())"))
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        $binDir = Join-Path $fixture 'bin'
        [void][System.IO.Directory]::CreateDirectory($binDir)
        $wrapper = Join-Path $fixture 'dotfiles.ps1'
        Copy-Item -LiteralPath (Get-WrapperPath) -Destination $wrapper
        Copy-Item -LiteralPath $env:BINARY_PATH -Destination (Join-Path $binDir $binaryName)

        $output = & $wrapper --version 2>&1
        $text = ($output | Out-String)
        if ($LASTEXITCODE -eq 0 -and $text -match 'dotfiles\s+(\S+)') {
            $version = $Matches[1]
            Write-TestPass "Version detected via wrapper: $version"
            return $true
        } else {
            Write-TestFail "Version detection via wrapper failed (exit $LASTEXITCODE): $text"
            return $false
        }
    } catch {
        Write-TestFail "Version command via wrapper failed: $_"
        return $false
    } finally {
        foreach ($name in $savedEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
        }
        Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-ChecksumVerification {
    Write-TestStage "Testing the wrapper's checksum asset selection"
    try {
        $wrapper = Get-WrapperPath
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

function Test-ArgumentForwarding {
    Write-TestStage "Testing argument forwarding through wrapper"

    if (-not $env:BINARY_PATH -or -not (Test-Path -LiteralPath $env:BINARY_PATH -PathType Leaf)) {
        Write-Information "Skipping: BINARY_PATH not set or binary not found" -InformationAction Continue
        return $true
    }

    $isWindowsPlatform = ($IsWindows -or ($null -eq $IsWindows -and $env:OS -eq 'Windows_NT'))
    $binaryName = if ($isWindowsPlatform) { 'dotfiles.exe' } else { 'dotfiles' }
    $fixture = [System.IO.Path]::GetFullPath(
        (Join-Path (Get-RepoRoot) ".wrapper-help-$([guid]::NewGuid())"))
    $savedEnvironment = @{}
    foreach ($name in @('DOTFILES_ROOT', 'DOTFILES_WRAPPER')) {
        $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        $binDir = Join-Path $fixture 'bin'
        [void][System.IO.Directory]::CreateDirectory($binDir)
        $wrapper = Join-Path $fixture 'dotfiles.ps1'
        Copy-Item -LiteralPath (Get-WrapperPath) -Destination $wrapper
        Copy-Item -LiteralPath $env:BINARY_PATH -Destination (Join-Path $binDir $binaryName)

        $output = & $wrapper --help 2>&1
        $text = ($output | Out-String)
        if ($LASTEXITCODE -eq 0 -and $text -match 'Usage:') {
            Write-TestPass "Arguments forwarded correctly through wrapper"
            return $true
        } else {
            Write-TestFail "Argument forwarding via wrapper failed (exit $LASTEXITCODE): $text"
            return $false
        }
    } catch {
        Write-TestFail "Argument forwarding test failed: $_"
        return $false
    } finally {
        foreach ($name in $savedEnvironment.Keys) {
            [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
        }
        Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Test-UpdateArgumentForwarding {
    Write-TestStage "Testing update argument forwarding through wrapper"

    $wrapper = Get-WrapperPath
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

    $wrapper = Get-WrapperPath
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

function Test-VersionPinnedBootstrapUrl {
    Write-TestStage "Testing wrapper resolves release tag and uses pinned URLs for binary and checksum"

    $wrapper = Get-WrapperPath
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
