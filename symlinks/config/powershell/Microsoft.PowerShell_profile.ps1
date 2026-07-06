# Global variables justified: profile file maintains session state
[Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidGlobalVars", "")]
param()

if (Get-Module PSReadLine)
{
    Set-PSReadLineOption -BellStyle None
    Set-PSReadLineOption -HistorySearchCursorMovesToEnd
    Set-PSReadLineOption -MaximumHistoryCount 10000
    Set-PSReadLineOption -Colors @{
        "Command"   = "#7aa2f7"
        "Comment"   = "#565f89"
        "Default"   = "#c0caf5"
        "Emphasis"  = "#bb9af7"
        "Error"     = "#f7768e"
        "Keyword"   = "#bb9af7"
        "Member"    = "#7dcfff"
        "Number"    = "#ff9e64"
        "Operator"  = "#7dcfff"
        "Parameter" = "#e0af68"
        "String"    = "#9ece6a"
        "Type"      = "#7dcfff"
        "Variable"  = "#7aa2f7"
    }
}

$Global:GitExists = [bool](Get-Command "git" -ErrorAction SilentlyContinue)

if (Get-Command "code-insiders" -ErrorAction SilentlyContinue)
{
    Set-Alias -Name code -Value code-insiders
}

Set-Alias -Name dot -Value dotfiles

$Global:IsNestedPwsh = $false
if ($null -eq $env:windir)
{
    $pwshCmd = Get-Command "pwsh" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -ne $pwshCmd -and $pwshCmd.Path -ne $env:SHELL)
    {
        $Global:IsNestedPwsh = $true
    }
}

function Prompt
{
    param (
    )

    $origLastExitCode = $LASTEXITCODE

    $esc = [char]27
    $reset = "$esc[0m"
    $blue = "$esc[38;2;122;162;247m"
    $cyan = "$esc[38;2;125;207;255m"
    $foreground = "$esc[38;2;192;202;245m"
    $red = "$esc[38;2;247;118;142m"
    $yellow = "$esc[38;2;224;175;104m"

    if ($Global:IsNestedPwsh)
    {
        [Console]::Write("${cyan}pwsh ${reset}")
    }

    $curPath = $ExecutionContext.SessionState.Path.CurrentLocation.Path

    if ($curPath.StartsWith($Home, [System.StringComparison]::OrdinalIgnoreCase))
    {
        $curPath = "~" + $curPath.SubString($Home.Length)
    }

    [Console]::Write("${yellow}${curPath}${reset}")

    if ($Global:GitExists)
    {
        # Performance: Use --porcelain=v1 and --untracked-files=no for faster git status
        $status = @(git --no-optional-locks status --short --branch --porcelain=v1 --untracked-files=no 2> $null)

        if ($status.Count -gt 0)
        {
            $branchLine = $status[0]
            if ($branchLine.StartsWith("## "))
            {
                $branchName = $branchLine.Substring(3)
                $ellipsis = $branchName.IndexOf("...")
                if ($ellipsis -gt 0) { $branchName = $branchName.Substring(0, $ellipsis) }

                [Console]::Write("${foreground} ${branchName}${reset}")

                $changes = $status.Count - 1
                if ($changes -gt 0)
                {
                    [Console]::Write("${red}+$changes${reset}")
                }
            }
        }
    }

    [Console]::WriteLine("")

    if ($env:username -eq "root")
    {
        "${red}# ${reset}"
    }
    else
    {
        "${blue}$ ${reset}"
    }

    $LASTEXITCODE = $origLastExitCode
}

# AI / GitHub Copilot CLI aliases
if (Get-Command "gh" -ErrorAction SilentlyContinue)
{
    function Invoke-CopilotChat
    {
        if ($args.Count -eq 0)
        {
            gh copilot -- --yolo
        }
        else
        {
            $previousGhPromptDisabled = $env:GH_PROMPT_DISABLED
            $previousGitTerminalPrompt = $env:GIT_TERMINAL_PROMPT

            try
            {
                $env:GH_PROMPT_DISABLED = "1"
                $env:GIT_TERMINAL_PROMPT = "0"
                gh copilot -- --yolo -p ($args -join ' ')
            }
            finally
            {
                if ($null -eq $previousGhPromptDisabled)
                {
                    Remove-Item Env:\GH_PROMPT_DISABLED -ErrorAction SilentlyContinue
                }
                else
                {
                    $env:GH_PROMPT_DISABLED = $previousGhPromptDisabled
                }

                if ($null -eq $previousGitTerminalPrompt)
                {
                    Remove-Item Env:\GIT_TERMINAL_PROMPT -ErrorAction SilentlyContinue
                }
                else
                {
                    $env:GIT_TERMINAL_PROMPT = $previousGitTerminalPrompt
                }
            }
        }
    }

    function Invoke-CopilotSuggest
    {
        gh copilot -i suggest $args
    }

    Set-Alias -Name ai -Value Invoke-CopilotChat
    Set-Alias -Name aic -Value Invoke-CopilotSuggest
}

function Add-PathEntry
{
    param (
        [string]$Directory
    )

    if (-not (Test-Path $Directory))
    {
        return
    }

    $comparison = [System.StringComparison]::Ordinal
    if ($null -ne $env:windir)
    {
        $comparison = [System.StringComparison]::OrdinalIgnoreCase
    }

    $trimChars = [char[]]@(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $normalizedDirectory = $Directory.TrimEnd($trimChars)

    foreach ($entry in @($env:Path -split [System.IO.Path]::PathSeparator))
    {
        if ([string]::IsNullOrWhiteSpace($entry))
        {
            continue
        }

        if ([string]::Equals($entry.TrimEnd($trimChars), $normalizedDirectory, $comparison))
        {
            return
        }
    }

    if ([string]::IsNullOrEmpty($env:Path))
    {
        $env:Path = $Directory
    }
    else
    {
        $env:Path += "$([System.IO.Path]::PathSeparator)$Directory"
    }
}

# Ensure local bin directory is in PATH
$localBinDir = Join-Path (Join-Path $HOME ".local") "bin"
Add-PathEntry -Directory $localBinDir

# Ensure Cargo (Rust) bin directory is in PATH
$cargoDir = Join-Path (Join-Path $HOME ".cargo") "bin"
Add-PathEntry -Directory $cargoDir

# Load additional profile scripts from profile.d directory
$profileDir = Join-Path $HOME ".config\powershell\profile.d"
if (Test-Path $profileDir)
{
    Get-ChildItem -Path $profileDir -Filter "*.ps1" -File |
        Sort-Object -Property FullName |
        ForEach-Object {
            . $_.FullName
        }
}
