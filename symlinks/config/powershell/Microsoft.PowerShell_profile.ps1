# Global variables justified: profile file maintains session state
[Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSAvoidGlobalVars", "")]
[Diagnostics.CodeAnalysis.SuppressMessageAttribute(
    "PSUseApprovedVerbs",
    "dot",
    Justification = "dot is the established short form of the dotfiles command"
)]
param()

function Add-PathEntry
{
    param (
        [string]$Directory,
        [switch]$Prepend
    )

    if (-not (Test-Path -LiteralPath $Directory -PathType Container))
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
    $normalizedDirectory = $Directory.Replace(
        [IO.Path]::AltDirectorySeparatorChar, [IO.Path]::DirectorySeparatorChar
    ).TrimEnd($trimChars)

    $entries = @($env:PATH -split [System.IO.Path]::PathSeparator | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $remaining = [System.Collections.Generic.List[string]]::new()
    $found = $false
    $firstMatches = $false
    for ($i = 0; $i -lt $entries.Count; $i++)
    {
        $entry = $entries[$i]
        $normalizedEntry = $entry.Replace(
            [IO.Path]::AltDirectorySeparatorChar, [IO.Path]::DirectorySeparatorChar
        ).TrimEnd($trimChars)
        if ([string]::Equals($normalizedEntry, $normalizedDirectory, $comparison))
        {
            if ($i -eq 0)
            {
                $firstMatches = $true
            }
            $found = $true
            continue
        }
        $remaining.Add($entry)
    }

    if ($Prepend)
    {
        if ($firstMatches -and $remaining.Count -eq ($entries.Count - 1))
        {
            return
        }
        $newEntries = @($Directory) + @($remaining)
        $env:PATH = $newEntries -join [System.IO.Path]::PathSeparator
        return
    }

    if ($found)
    {
        return
    }

    if ([string]::IsNullOrEmpty($env:PATH))
    {
        $env:PATH = $Directory
    }
    else
    {
        $env:PATH += "$([System.IO.Path]::PathSeparator)$Directory"
    }
}

# Discover user-installed commands before configuring optional integrations.
$cargoHome = $env:CARGO_HOME
if ([string]::IsNullOrEmpty($cargoHome))
{
    $cargoHome = Join-Path $HOME ".cargo"
}
Add-PathEntry -Directory (Join-Path $cargoHome "bin") -Prepend

$localBinDir = Join-Path (Join-Path $HOME ".local") "bin"
Add-PathEntry -Directory $localBinDir -Prepend

if (Get-Module PSReadLine)
{
    Set-PSReadLineOption -BellStyle None
    Set-PSReadLineOption -HistorySearchCursorMovesToEnd
    Set-PSReadLineOption -MaximumHistoryCount 100000
    # Older bundled PSReadLine versions lack some display options.
    $psReadLineParameters = (Get-Command Set-PSReadLineOption).Parameters
    if ($psReadLineParameters.ContainsKey('PredictionSource'))
    {
        Set-PSReadLineOption -PredictionSource History
    }
    if ($psReadLineParameters.ContainsKey('PredictionViewStyle'))
    {
        Set-PSReadLineOption -PredictionViewStyle InlineView
    }
    if ($psReadLineParameters.ContainsKey('Colors'))
    {
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
}

$Global:GitExists = [bool](Get-Command "git" -ErrorAction SilentlyContinue)

function Test-DotfilesAliasWritable
{
    param([string] $Name)

    $existing = Get-Alias -Name $Name -ErrorAction SilentlyContinue
    $protected = [System.Management.Automation.ScopedItemOptions]'ReadOnly, Constant'
    return $null -eq $existing -or -not ($existing.Options -band $protected)
}

if ((Get-Command "code-insiders" -ErrorAction SilentlyContinue) -and (Test-DotfilesAliasWritable 'code'))
{
    Set-Alias -Name code -Value code-insiders
}

function dot
{
    dotfiles @args
}

function Format-PromptPath
{
    param(
        [string] $Path,
        [string] $HomeDirectory,
        [bool] $WindowsPlatform = ([IO.Path]::DirectorySeparatorChar -eq '\')
    )

    if ([string]::IsNullOrEmpty($HomeDirectory))
    {
        return $Path
    }

    $comparison = [StringComparison]::Ordinal
    $separator = '/'
    if ($WindowsPlatform)
    {
        $comparison = [StringComparison]::OrdinalIgnoreCase
        $separator = '\'
        $Path = $Path.Replace('/', '\')
        $HomeDirectory = $HomeDirectory.Replace('/', '\')
    }

    $homePath = $HomeDirectory.TrimEnd([char]$separator)
    if ([string]::Equals($Path.TrimEnd([char]$separator), $homePath, $comparison))
    {
        return '~'
    }
    if ($Path.StartsWith($homePath + $separator, $comparison))
    {
        return '~' + $separator + $Path.Substring($homePath.Length + 1)
    }
    return $Path
}

function Test-PromptRoot
{
    param(
        [string] $UserName = [Environment]::UserName,
        [bool] $WindowsPlatform = ([IO.Path]::DirectorySeparatorChar -eq '\')
    )

    return -not $WindowsPlatform -and $UserName -ceq 'root'
}

function Resolve-ExecutablePath
{
    param (
        [string]$Path
    )

    if ([string]::IsNullOrWhiteSpace($Path))
    {
        return $Path
    }

    try
    {
        $info = [System.IO.FileInfo]::new($Path)
        if ($info.Exists -and ($info | Get-Member -Name 'ResolveLinkTarget' -MemberType Method))
        {
            $target = $info.ResolveLinkTarget($true)
            if ($null -ne $target)
            {
                return $target.FullName
            }
            if ($null -ne $info.Directory)
            {
                $dirTarget = $info.Directory.ResolveLinkTarget($true)
                if ($null -ne $dirTarget)
                {
                    return Join-Path $dirTarget.FullName $info.Name
                }
            }
        }
        return $info.FullName
    }
    catch
    {
        return $Path
    }
}

$Global:IsNestedPwsh = $false
if ($null -eq $env:windir)
{
    $pwshCmd = Get-Command "pwsh" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -ne $pwshCmd)
    {
        if ([string]::IsNullOrEmpty($env:SHELL))
        {
            $Global:IsNestedPwsh = $true
        }
        else
        {
            $resolvedPwsh = Resolve-ExecutablePath -Path $pwshCmd.Path
            $resolvedShell = Resolve-ExecutablePath -Path $env:SHELL
            if ($resolvedPwsh -cne $resolvedShell)
            {
                $Global:IsNestedPwsh = $true
            }
        }
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

    $promptLine = ""
    if ($Global:IsNestedPwsh)
    {
        $promptLine += "${cyan}pwsh ${reset}"
    }

    $curPath = Format-PromptPath -Path $ExecutionContext.SessionState.Path.CurrentLocation.Path -HomeDirectory $HOME

    $promptLine += "${yellow}${curPath}${reset}"

    if ($Global:GitExists)
    {
        try
        {
            $branchName = git rev-parse --abbrev-ref HEAD 2> $null
            if ($LASTEXITCODE -ne 0)
            {
                $branchName = $null
            }
        }
        catch
        {
            # Native-command errors can be terminating, including outside a worktree.
            $branchName = $null
        }
        finally
        {
            $global:LASTEXITCODE = $origLastExitCode
        }
        if (-not [string]::IsNullOrWhiteSpace($branchName))
        {
            $promptLine += "${foreground} ${branchName}${reset}"
        }
    }

    if (Test-PromptRoot)
    {
        $promptSuffix = "${red}# ${reset}"
    }
    else
    {
        $promptSuffix = "${blue}$ ${reset}"
    }

    return "$promptLine`n$promptSuffix"
}

# AI CLI aliases
if ((Get-Command "codex" -ErrorAction SilentlyContinue) -and (Test-DotfilesAliasWritable 'ai'))
{
    function Invoke-CodexChat
    {
        codex @args
    }

    Set-Alias -Name ai -Value Invoke-CodexChat
}

# Load additional profile scripts from profile.d directory
$profileDir = Join-Path $HOME ".config\powershell\profile.d"
if (Test-Path -LiteralPath $profileDir -PathType Container)
{
    Get-ChildItem -LiteralPath $profileDir -Filter "*.ps1" -File |
        Sort-Object -Property FullName |
        ForEach-Object {
            $fragment = $_.FullName
            try
            {
                . $fragment
            }
            catch
            {
                Write-Warning "Could not load PowerShell profile fragment '${fragment}': $_"
            }
        }
}
