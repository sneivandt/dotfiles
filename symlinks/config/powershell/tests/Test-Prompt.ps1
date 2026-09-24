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
$promptDefinition = $ast.Find({
        param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Prompt'
    }, $true)
if ($null -eq $promptDefinition)
{
    throw 'The profile must define Prompt.'
}
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
    # Load only Prompt, not the profile's PATH, aliases, or installed extensions.
    $setup = $promptDefinition.Extent.Text + "`n" +
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
