$ErrorActionPreference = 'Stop'

$profilePath = Join-Path (Split-Path $PSScriptRoot) 'Microsoft.PowerShell_profile.ps1'
$repositoryRoot = [IO.Path]::GetFullPath([IO.Path]::Combine($PSScriptRoot, '..', '..', '..', '..'))
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($profilePath, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0)
{
    throw "Profile parse errors: $parseErrors"
}
$definitions = @{}
foreach ($name in @('Prompt', 'Format-PromptPath', 'Test-PromptRoot', 'dot', 'Add-PathEntry'))
{
    $definition = $ast.Find({
            param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name
        }, $true)
    if ($null -eq $definition)
    {
        throw "The profile must define $name."
    }
    $definitions[$name] = $definition.Extent.Text
}
$promptSetup = @('Format-PromptPath', 'Test-PromptRoot', 'Prompt') |
    ForEach-Object { $definitions[$_] }
if (-not (Get-Command git -ErrorAction SilentlyContinue))
{
    throw 'These prompt tests require Git.'
}

$cases = @(
    @{ Name = 'successful Git query'; ExitCode = 42; GitExists = $true; InvalidGitDirectory = $false; ThrowFromGit = $false }
    @{ Name = 'failed Git query'; ExitCode = 0; GitExists = $true; InvalidGitDirectory = $true; ThrowFromGit = $false }
    @{ Name = 'Git unavailable'; ExitCode = 42; GitExists = $false; InvalidGitDirectory = $false; ThrowFromGit = $false }
    @{ Name = 'terminating Git error'; ExitCode = 42; GitExists = $true; InvalidGitDirectory = $false; ThrowFromGit = $true }
)

foreach ($case in $cases)
{
    # Load only prompt functions, not the profile's PATH, aliases, or extensions.
    $setup = ($promptSetup -join "`n") + "`n" +
    '$Global:IsNestedPwsh = $false; $Global:GitExists = $' + $case.GitExists.ToString().ToLowerInvariant() +
    '; $global:LASTEXITCODE = ' + $case.ExitCode
    if ($case.ThrowFromGit)
    {
        $setup += "`n" + 'function git { $global:LASTEXITCODE = 7; throw "Synthetic Git failure" }'
    }

    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = (Get-Process -Id $PID).Path
    $start.WorkingDirectory = $repositoryRoot
    $start.UseShellExecute = $false
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.Environment.Remove('GIT_DIR') | Out-Null
    $start.Environment.Remove('GIT_WORK_TREE') | Out-Null
    $start.Environment['GIT_CONFIG_NOSYSTEM'] = '1'
    $start.Environment['GIT_CONFIG_GLOBAL'] = ''
    if ($case.InvalidGitDirectory)
    {
        $start.Environment['GIT_DIR'] = Join-Path $repositoryRoot ('.missing-git-' + [Guid]::NewGuid())
    }
    foreach ($argument in @('-NoLogo', '-NoProfile', '-NoExit', '-EncodedCommand',
            [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($setup))))
    {
        $start.ArgumentList.Add($argument)
    }

    $process = [Diagnostics.Process]::Start($start)
    try
    {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        # The host renders Prompt before reading this next interactive command.
        $process.StandardInput.WriteLine('Write-Output ("PROMPT_EXITCODE:" + $global:LASTEXITCODE)')
        $process.StandardInput.WriteLine('exit 0')
        $process.StandardInput.Close()
        if (-not $process.WaitForExit(15000))
        {
            $process.Kill($true)
            throw "Prompt test timed out: $($case.Name)"
        }
        $output = $stdout.GetAwaiter().GetResult()
        $errors = $stderr.GetAwaiter().GetResult()
        $expected = "PROMPT_EXITCODE:$($case.ExitCode)"
        $plainOutput = [regex]::Replace($output, '\x1b\[[0-?]*[ -/]*[@-~]', '')
        if ($process.ExitCode -ne 0 -or $expected -notin ($plainOutput -split '\r?\n'))
        {
            throw "Prompt test failed: $($case.Name)`nExpected $expected`n$output`n$errors"
        }
        Write-Output "PASS: $($case.Name)"
    }
    finally
    {
        $process.Dispose()
    }
}

& {
    foreach ($definition in $definitions.Values)
    {
        . ([scriptblock]::Create($definition))
    }

    $pathCases = @(
        @{ Path = '/home/alex'; HomeDirectory = '/home/alex'; WindowsPlatform = $false; Expected = '~' }
        @{ Path = '/home/alex/src'; HomeDirectory = '/home/alex'; WindowsPlatform = $false; Expected = '~/src' }
        @{ Path = '/home/alexander'; HomeDirectory = '/home/alex'; WindowsPlatform = $false; Expected = '/home/alexander' }
        @{ Path = '/home/Alex/src'; HomeDirectory = '/home/alex'; WindowsPlatform = $false; Expected = '/home/Alex/src' }
        @{ Path = '/home/alex/src'; HomeDirectory = '/home/alex/'; WindowsPlatform = $false; Expected = '~/src' }
        @{ Path = '/'; HomeDirectory = '/'; WindowsPlatform = $false; Expected = '~' }
        @{ Path = '/src'; HomeDirectory = '/'; WindowsPlatform = $false; Expected = '~/src' }
        @{ Path = '/src'; HomeDirectory = ''; WindowsPlatform = $false; Expected = '/src' }
        @{ Path = 'C:\Users\Alex'; HomeDirectory = 'c:\users\alex'; WindowsPlatform = $true; Expected = '~' }
        @{ Path = 'C:\Users\Alex\src'; HomeDirectory = 'c:/users/alex/'; WindowsPlatform = $true; Expected = '~\src' }
        @{ Path = 'C:\Users\Alexander'; HomeDirectory = 'C:\Users\Alex'; WindowsPlatform = $true; Expected = 'C:\Users\Alexander' }
        @{ Path = 'C:\'; HomeDirectory = 'C:\'; WindowsPlatform = $true; Expected = '~' }
        @{ Path = 'C:\src'; HomeDirectory = 'C:\'; WindowsPlatform = $true; Expected = '~\src' }
    )
    foreach ($case in $pathCases)
    {
        $actual = Format-PromptPath -Path $case.Path -HomeDirectory $case.HomeDirectory -WindowsPlatform $case.WindowsPlatform
        if ($actual -cne $case.Expected)
        {
            throw "Path formatting failed: $($case.Path) => $actual; expected $($case.Expected)"
        }
    }
    Write-Output "PASS: $($pathCases.Count) platform-specific home-path cases"

    foreach ($case in @(
            @{ UserName = 'root'; WindowsPlatform = $false; Expected = $true }
            @{ UserName = 'Root'; WindowsPlatform = $false; Expected = $false }
            @{ UserName = 'alex'; WindowsPlatform = $false; Expected = $false }
            @{ UserName = 'root'; WindowsPlatform = $true; Expected = $false }
        ))
    {
        if ((Test-PromptRoot -UserName $case.UserName -WindowsPlatform $case.WindowsPlatform) -ne $case.Expected)
        {
            throw "Root detection failed: $($case.UserName), Windows=$($case.WindowsPlatform)"
        }
    }
    $expectedRoot = [IO.Path]::DirectorySeparatorChar -eq '/' -and [Environment]::UserName -ceq 'root'
    $originalUser = $env:USER
    $originalUsername = $env:USERNAME
    try
    {
        $env:USER = 'someone-else'
        $env:USERNAME = 'root'
        if ((Test-PromptRoot) -ne $expectedRoot)
        {
            throw 'Root detection must use the process identity, not a username environment variable.'
        }
    }
    finally
    {
        $env:USER = $originalUser
        $env:USERNAME = $originalUsername
    }
    Write-Output 'PASS: Unix root and Windows non-root identity cases'

    & {
        $originalVariables = @{}
        foreach ($name in @('GitExists', 'IsNestedPwsh', 'LASTEXITCODE'))
        {
            $variable = Get-Variable -Name $name -Scope Global -ErrorAction SilentlyContinue
            $originalVariables[$name] = @{ Exists = $null -ne $variable; Value = $variable.Value }
        }
        try
        {
            Set-Variable -Name GitExists -Value $true -Scope Global
            Set-Variable -Name IsNestedPwsh -Value $false -Scope Global
            $Global:LASTEXITCODE = 42
            function git { $global:LASTEXITCODE = 128; 'HEAD' }
            $text = Prompt
            if ($text.Contains('HEAD') -or $Global:LASTEXITCODE -ne 42)
            {
                throw 'A failed Git query must not display its stdout as a branch or change LASTEXITCODE.'
            }
            Write-Output 'PASS: failed Git stdout is excluded from the prompt'
        }
        finally
        {
            # GitHub's pwsh wrapper exits with LASTEXITCODE after dot-sourcing.
            foreach ($entry in $originalVariables.GetEnumerator())
            {
                if (-not $entry.Value.Exists)
                {
                    Remove-Variable -Name $entry.Key -Scope Global -ErrorAction SilentlyContinue
                }
                else
                {
                    Set-Variable -Name $entry.Key -Scope Global -Value $entry.Value.Value
                }
            }
        }
    }

    $pathFixture = Join-Path $repositoryRoot ('.dotfiles-path-' + [Guid]::NewGuid())
    $originalPath = $env:PATH
    try
    {
        $directory = Join-Path $pathFixture '[literal] bin'
        [IO.Directory]::CreateDirectory($directory) | Out-Null
        $file = Join-Path $pathFixture 'ordinary-file'
        [IO.File]::WriteAllText($file, 'fixture')
        $env:PATH = ''
        Add-PathEntry -Directory $file
        if (-not [string]::IsNullOrEmpty($env:PATH))
        {
            throw 'Ordinary files must not be added to PATH.'
        }
        Add-PathEntry -Directory $directory
        Add-PathEntry -Directory $directory
        if ($env:PATH -cne $directory)
        {
            throw 'Literal directory names containing brackets must be added exactly once.'
        }
        $alternateDirectory = $directory.Replace(
            [IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar
        ) + [IO.Path]::AltDirectorySeparatorChar
        $env:PATH = $alternateDirectory
        Add-PathEntry -Directory $directory
        if ($env:PATH -cne $alternateDirectory)
        {
            throw 'Equivalent directory separators and trailing slashes must not duplicate PATH entries.'
        }
        Write-Output 'PASS: literal PATH directories, file rejection and idempotency'
    }
    finally
    {
        $env:PATH = $originalPath
        if ([IO.Directory]::Exists($pathFixture))
        {
            Remove-Item -LiteralPath $pathFixture -Recurse -Force
        }
    }

    $profileFixture = Join-Path $repositoryRoot ('.powershell-profile-' + [Guid]::NewGuid())
    try
    {
        $fixtureHome = Join-Path $profileFixture '[literal] home'
        $localBin = Join-Path $fixtureHome '.local\bin'
        $defaultCargo = Join-Path $fixtureHome '.cargo'
        $customCargo = Join-Path $profileFixture 'custom cargo'
        $extensions = Join-Path $fixtureHome '.config\powershell\profile.d'
        foreach ($directory in @($localBin, (Join-Path $defaultCargo 'bin'), (Join-Path $customCargo 'bin'), $extensions))
        {
            [IO.Directory]::CreateDirectory($directory) | Out-Null
        }
        foreach ($commandPath in @(
                (Join-Path $localBin 'git'),
                (Join-Path $localBin 'code-insiders'),
                (Join-Path $defaultCargo 'bin\codex'),
                (Join-Path $customCargo 'bin\codex')
            ))
        {
            if ([IO.Path]::DirectorySeparatorChar -eq '\')
            {
                [IO.File]::WriteAllText("$commandPath.ps1", "'fixture'")
            }
            else
            {
                # Unix command lookup does not infer .ps1 extensions.
                [IO.File]::WriteAllText($commandPath, "#!/bin/sh`nexit 0`n")
                [IO.File]::SetUnixFileMode($commandPath,
                    [IO.UnixFileMode]::UserRead -bor [IO.UnixFileMode]::UserWrite -bor [IO.UnixFileMode]::UserExecute)
            }
        }
        [IO.File]::WriteAllText((Join-Path $extensions '20-second.ps1'), '$script:ExtensionOrder += "second"')
        [IO.File]::WriteAllText((Join-Path $extensions '10-first.ps1'), '$script:ExtensionOrder += "first"')
        $startupTest = {
            $ErrorActionPreference = 'Stop'
            Set-Variable -Name HOME -Value $env:PROFILE_TEST_HOME -Force
            $env:PATH = ''
            $script:ExtensionOrder = @()
            . $env:PROFILE_TEST_SOURCE
            if (-not (Get-Variable -Name GitExists -Scope Global -ValueOnly) -or (Get-Alias code).Definition -ne 'code-insiders' -or
                (Get-Alias ai).Definition -ne 'Invoke-CodexChat')
            {
                throw 'First startup must detect integrations installed in user bin directories.'
            }
            $expected = @(
                (Join-Path $HOME '.local\bin'),
                (Join-Path $env:PROFILE_TEST_CARGO 'bin')
            )
            if (($env:PATH -split [IO.Path]::PathSeparator | ConvertTo-Json -Compress) -cne
                ($expected | ConvertTo-Json -Compress))
            {
                throw "Startup did not honor the selected Cargo home: $env:PATH"
            }
            if (($script:ExtensionOrder -join ',') -ne 'first,second')
            {
                throw 'Profile fragments beneath literal bracket paths must load in sorted order.'
            }
            $initialPath = $env:PATH
            . $env:PROFILE_TEST_SOURCE
            if ($env:PATH -cne $initialPath)
            {
                throw 'Reloading the profile duplicated user bin directories.'
            }
        }.ToString()
        foreach ($cargoHome in @('', $customCargo))
        {
            $start = [Diagnostics.ProcessStartInfo]::new()
            $start.FileName = (Get-Process -Id $PID).Path
            $start.WorkingDirectory = $repositoryRoot
            $start.UseShellExecute = $false
            $start.RedirectStandardOutput = $true
            $start.RedirectStandardError = $true
            $start.Environment['PATH'] = ''
            $start.Environment['CARGO_HOME'] = $cargoHome
            $start.Environment['PROFILE_TEST_HOME'] = $fixtureHome
            $start.Environment['PROFILE_TEST_SOURCE'] = $profilePath
            $start.Environment['PROFILE_TEST_CARGO'] = if ($cargoHome) { $cargoHome } else { $defaultCargo }
            foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand',
                    [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($startupTest))))
            {
                $start.ArgumentList.Add($argument)
            }
            $process = [Diagnostics.Process]::Start($start)
            try
            {
                $stdout = $process.StandardOutput.ReadToEndAsync()
                $stderr = $process.StandardError.ReadToEndAsync()
                if (-not $process.WaitForExit(15000))
                {
                    $process.Kill($true)
                    throw 'Isolated profile startup timed out.'
                }
                $output = $stdout.GetAwaiter().GetResult()
                $errors = $stderr.GetAwaiter().GetResult()
                if ($process.ExitCode -ne 0)
                {
                    throw "Isolated profile startup failed (CARGO_HOME='$cargoHome'):`n$output`n$errors"
                }
            }
            finally
            {
                $process.Dispose()
            }
        }
        Write-Output 'PASS: first-start integration discovery, custom/default Cargo home, literal profile fragments and reload'
    }
    finally
    {
        if ([IO.Directory]::Exists($profileFixture))
        {
            Remove-Item -LiteralPath $profileFixture -Recurse -Force
        }
    }

    Set-Item Function:dotfiles -Value { , $args }
    $actual = dot update -v
    if (($actual | ConvertTo-Json -Compress) -cne '["update","-v"]')
    {
        throw 'dot must not consume -v as a PowerShell common parameter.'
    }
    foreach ($arguments in @(
            @('update', '-v'),
            @('update', '--verbose', '--dry-run'),
            @('-d', '--', '--help', 'two words', '"quoted"', '', [string][char]0x03BB)
        ))
    {
        $actual = dot @arguments
        if (($actual | ConvertTo-Json -Compress) -cne ($arguments | ConvertTo-Json -Compress))
        {
            throw "dot changed the native arguments: $($actual | ConvertTo-Json -Compress)"
        }
    }
    Write-Output 'PASS: transparent dot forwarding, including -v, option terminator and empty arguments'

    # Exercise the production registration without building or invoking the CLI.
    $completionSource = Get-Content -Raw (Join-Path $repositoryRoot 'cli/src/app/completion.rs')
    $registration = [regex]::Match(
        $completionSource,
        '(?s)const POWERSHELL_DOT_COMPLETER: &str = r"(?<script>.*?)";'
    )
    if (-not $registration.Success)
    {
        throw 'The production dot completion registration was not found.'
    }
    $completionErrors = $null
    $completionAst = [Management.Automation.Language.Parser]::ParseInput(
        $registration.Groups['script'].Value, [ref]$null, [ref]$completionErrors
    )
    if ($completionErrors.Count -gt 0)
    {
        throw "Completion registration parse errors: $completionErrors"
    }
    . ($completionAst.GetScriptBlock())
    Set-Item Function:dotfiles -Value { throw 'Completion must not invoke dotfiles.' }
    Register-ArgumentCompleter -Native -CommandName dotfiles -ScriptBlock {
        param($wordToComplete)

        if ($wordToComplete -eq '' -or $wordToComplete -like 'upd*')
        {
            [Management.Automation.CompletionResult]::new('update', 'update', 'ParameterValue', 'Mock update')
        }
        if ($wordToComplete -like '--on*')
        {
            [Management.Automation.CompletionResult]::new('--only', '--only', 'ParameterName', 'Mock selector')
        }
        if ($wordToComplete -like '--ver*')
        {
            [Management.Automation.CompletionResult]::new('--verbose', '--verbose', 'ParameterName', 'Mock verbosity')
        }
    }
    foreach ($case in @(
            @{ Text = 'dot upd'; Expected = 'update' }
            @{ Text = 'dot update --on'; Expected = '--only' }
            @{ Text = 'dot update --ver'; Expected = '--verbose' }
            @{ Text = 'Write-Output ignored; dot upd'; Expected = 'update' }
            @{ Text = 'dot upd trailing'; Cursor = 'dot upd'.Length; Expected = 'update' }
            @{ Text = 'dot '; Expected = 'update' }
        ))
    {
        $cursorColumn = if ($case.ContainsKey('Cursor')) { $case.Cursor } else { $case.Text.Length }
        $completionMatches = (TabExpansion2 -InputScript $case.Text -CursorColumn $cursorColumn).CompletionMatches
        if ($completionMatches.Count -ne 1 -or $completionMatches[0].CompletionText -cne $case.Expected)
        {
            throw "Native dot completion failed: $($case.Text), cursor $cursorColumn"
        }
    }
    Write-Output 'PASS: native dot completion, command offsets, mid-line cursor and trailing space'
}
