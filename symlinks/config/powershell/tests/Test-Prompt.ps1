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
foreach ($name in @('Prompt', 'Format-PromptPath', 'Test-PromptRoot', 'dot'))
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

    Set-Item Function:dotfiles -Value { , $args }
    $actual = dot install -v
    if (($actual | ConvertTo-Json -Compress) -cne '["install","-v"]')
    {
        throw 'dot must not consume -v as a PowerShell common parameter.'
    }
    foreach ($arguments in @(
            @('install', '-v'),
            @('install', '--verbose', '--dry-run'),
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

        if ($wordToComplete -eq '' -or $wordToComplete -like 'ins*')
        {
            [Management.Automation.CompletionResult]::new('install', 'install', 'ParameterValue', 'Mock install')
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
            @{ Text = 'dot ins'; Expected = 'install' }
            @{ Text = 'dot install --on'; Expected = '--only' }
            @{ Text = 'dot install --ver'; Expected = '--verbose' }
            @{ Text = 'Write-Output ignored; dot ins'; Expected = 'install' }
            @{ Text = 'dot ins trailing'; Cursor = 'dot ins'.Length; Expected = 'install' }
            @{ Text = 'dot '; Expected = 'install' }
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
