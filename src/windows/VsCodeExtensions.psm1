<#
.SYNOPSIS
    VS Code extension management for Windows dotfiles
.DESCRIPTION
    Installs VS Code extensions from configuration file, supporting both
    stable (code) and insiders (code-insiders) editions. Supports profile-based
    filtering to install only extensions relevant to the active profile.
.NOTES
    Admin: Not required
#>

function Install-VsCodeExtensions
{
    <#
    .SYNOPSIS
        Install VS Code Extensions
    .DESCRIPTION
        Reads VS Code extensions from conf/vscode-extensions.ini and ensures
        they are installed. Checks both code and code-insiders binaries if
        available. Supports profile-based sections for filtering extensions
        by category (e.g., [base], [windows]).
    .PARAMETER root
        Repository root directory
    .PARAMETER excludedCategories
        Comma-separated list of categories to exclude (from profile)
    .PARAMETER DryRun
        When specified, logs actions that would be taken without making modifications
    #>
    # Plural name justified: function installs multiple extensions as batch operation
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root,

        [Parameter(Mandatory = $false)]
        [string]
        $excludedCategories = "",

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    $configFile = Join-Path $root "conf\vscode-extensions.ini"
    Write-VerboseMessage "Reading VS Code extension configuration from: conf/vscode-extensions.ini"

    if (-not (Test-Path $configFile))
    {
        Write-VerboseMessage "Skipping VS Code extensions: no vscode-extensions.ini found"
        return
    }

    # Check if any sections match the active profile
    if (-not (Test-HasMatchingSections -FilePath $configFile -ExcludedCategories $excludedCategories))
    {
        Write-VerboseMessage "Skipping VS Code extensions: no sections match current profile"
        return
    }

    Write-ProgressMessage -Message "Checking VS Code extensions..."

    # Get all sections from the config file
    $content = Get-Content $configFile
    $sections = @()
    foreach ($line in $content)
    {
        if ($line -match '^\[(.+)\]$')
        {
            $sections += $matches[1]
        }
    }

    Write-VerboseMessage "Found $($sections.Count) section(s) in vscode-extensions.ini: $($sections -join ', ')"

    $act = $false

    # Collect all extensions that should be installed based on profile
    $extensionsToInstall = @()
    foreach ($section in $sections)
    {
        Write-VerboseMessage "Processing extensions section: [$section]"

        # Check if this section should be included based on profile
        if (-not (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excludedCategories))
        {
            Write-VerboseMessage "Skipping VS Code extensions section [$section]: profile not included"
            continue
        }

        # Read extensions from this section
        $sectionExtensions = Read-IniSection -FilePath $configFile -SectionName $section
        Write-VerboseMessage "Found $($sectionExtensions.Count) extension(s) in section [$section]"
        $extensionsToInstall += $sectionExtensions
    }

    if ($extensionsToInstall.Count -eq 0)
    {
        Write-VerboseMessage "Skipping VS Code extensions: no extensions configured for current profile"
        return
    }

    # Remove duplicates if same extension appears in multiple sections
    $originalCount = $extensionsToInstall.Count
    $extensionsToInstall = $extensionsToInstall | Select-Object -Unique
    if ($extensionsToInstall.Count -lt $originalCount)
    {
        Write-VerboseMessage "Removed $($originalCount - $extensionsToInstall.Count) duplicate extension(s)"
    }
    Write-VerboseMessage "Total extensions to process: $($extensionsToInstall.Count)"

    # Iterate over both stable and insiders versions of VS Code
    foreach ($code in @('code', 'code-insiders'))
    {
        Write-VerboseMessage "Checking for VS Code binary: $code"

        # Check if the code binary exists
        if (-not (Get-Command $code -ErrorAction SilentlyContinue))
        {
            Write-VerboseMessage "Skipping $code`: not installed"
            continue
        }

        $act = $false

        # Get list of currently installed extensions to avoid redundant calls
        Write-VerboseMessage "Retrieving list of installed extensions for $code..."
        $installed = & $code --list-extensions
        Write-VerboseMessage "Found $($installed.Count) installed extension(s) for $code"

        foreach ($extension in $extensionsToInstall)
        {
            # Check if extension is already installed
            if ($installed -notcontains $extension)
            {
                if (-not $act)
                {
                    $act = $true
                    Write-Stage -Message "Installing $code Extensions"
                }

                if ($DryRun)
                {
                    Write-DryRunMessage -Message "Would install extension: $extension"
                    Add-Counter -CounterName "vscode_extensions_installed"
                }
                else
                {
                    Write-VerboseMessage "Installing extension: $extension"
                    $output = & $code --install-extension $extension 2>&1
                    if ($LASTEXITCODE -ne 0)
                    {
                        Write-Warning "Failed to install extension $extension for $code`: $output"
                    }
                    else
                    {
                        Add-Counter -CounterName "vscode_extensions_installed"
                    }
                }
            }
            else
            {
                Write-VerboseMessage "Skipping $code extension $extension`: already installed"
            }
        }
    }
}
Export-ModuleMember -Function Install-VsCodeExtensions
