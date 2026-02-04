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

    if (-not (Test-Path $configFile))
    {
        Write-Verbose "Skipping VS Code extensions: no vscode-extensions.ini found"
        return
    }

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

    if ($sections.Count -eq 0)
    {
        Write-Verbose "Skipping VS Code extensions: no sections found"
        return
    }

    # Collect all extensions that should be installed based on profile
    $extensionsToInstall = @()
    foreach ($section in $sections)
    {
        # Check if this section should be included based on profile
        if (-not (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excludedCategories))
        {
            Write-Verbose "Skipping VS Code extensions section [$section]: profile not included"
            continue
        }

        # Read extensions from this section
        $sectionExtensions = Read-IniSection -FilePath $configFile -SectionName $section
        $extensionsToInstall += $sectionExtensions
    }

    if ($extensionsToInstall.Count -eq 0)
    {
        Write-Verbose "Skipping VS Code extensions: no extensions configured for current profile"
        return
    }

    # Remove duplicates if same extension appears in multiple sections
    $extensionsToInstall = $extensionsToInstall | Select-Object -Unique

    # Iterate over both stable and insiders versions of VS Code
    foreach ($code in @('code', 'code-insiders'))
    {
        # Check if the code binary exists
        if (-not (Get-Command $code -ErrorAction SilentlyContinue))
        {
            Write-Verbose "Skipping $code`: not installed"
            # Clear LASTEXITCODE to prevent script exit with non-zero code
            $global:LASTEXITCODE = 0
            continue
        }

        $act = $false

        # Get list of currently installed extensions to avoid redundant calls
        $installed = & $code --list-extensions

        foreach ($extension in $extensionsToInstall)
        {
            # Check if extension is already installed
            if ($installed -notcontains $extension)
            {
                if (-not $act)
                {
                    $act = $true
                    Write-Output ":: Installing $code Extensions"
                }

                if ($DryRun)
                {
                    Write-Output "DRY-RUN: Would install extension: $extension"
                }
                else
                {
                    Write-Verbose "Installing extension: $extension"
                    $output = & $code --install-extension $extension 2>&1
                    if ($LASTEXITCODE -ne 0)
                    {
                        Write-Warning "Failed to install extension $extension for $code`: $output"
                    }
                }
            }
            else
            {
                Write-Verbose "Skipping $code extension $extension`: already installed"
            }
        }
    }
}
Export-ModuleMember -Function Install-VsCodeExtensions