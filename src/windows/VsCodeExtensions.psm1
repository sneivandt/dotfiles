#Requires -PSEdition Desktop

function Install-VsCodeExtensions
{
    <#
    .SYNOPSIS
        Install VS Code Extensions
    .DESCRIPTION
        Reads VS Code extensions from conf/vscode-extensions.ini and ensures
        they are installed. Supports both [extensions] section (all platforms)
        and profile-specific sections (e.g., [windows], [arch]). Checks both
        code and code-insiders binaries if available.
    .PARAMETER root
        Repository root directory
    .PARAMETER excludedCategories
        Comma-separated list of categories to exclude for profile filtering
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

    # Collect extensions from all applicable sections
    $allExtensions = @()

    # Get all sections from config file
    $content = Get-Content $configFile
    $sections = @()
    foreach ($line in $content)
    {
        $line = $line.Trim()
        if ($line -match '^\[(.+)\]$')
        {
            $sections += $matches[1]
        }
    }

    # Process each section that should be included
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
        $allExtensions += $sectionExtensions
    }

    # Remove duplicates
    $allExtensions = $allExtensions | Select-Object -Unique

    if ($allExtensions.Count -eq 0)
    {
        Write-Verbose "Skipping VS Code extensions: no extensions configured"
        return
    }

    # Iterate over both stable and insiders versions of VS Code
    foreach ($code in @('code', 'code-insiders'))
    {
        # Check if the code binary exists
        if (-not (Get-Command $code -ErrorAction SilentlyContinue))
        {
            Write-Verbose "Skipping $code`: not installed"
            continue
        }

        $act = $false

        # Get list of currently installed extensions to avoid redundant calls
        $installed = & $code --list-extensions

        foreach ($extension in $allExtensions)
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