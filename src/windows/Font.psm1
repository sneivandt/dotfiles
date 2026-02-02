#Requires -PSEdition Desktop
#Requires -RunAsAdministrator

function Install-Fonts
{
    <#
    .SYNOPSIS
        Install system fonts
    .DESCRIPTION
        Reads font families from conf/fonts.ini [fonts] section and ensures
        they are installed. Installs missing fonts using the extern/fonts
        installation script.
    .PARAMETER root
        Repository root directory
    .PARAMETER DryRun
        When specified, logs actions that would be taken without making modifications
    #>
    # Plural name justified: function installs multiple font families as batch operation
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root,

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    $configFile = Join-Path $root "conf\fonts.ini"

    if (-not (Test-Path $configFile))
    {
        Write-Verbose "Skipping fonts: no fonts.ini found"
        return
    }

    # Read font families from [fonts] section
    $fonts = Read-IniSection -FilePath $configFile -SectionName "fonts"

    if ($fonts.Count -eq 0)
    {
        Write-Verbose "Skipping fonts: no fonts configured"
        return
    }

    $act = $false

    foreach ($font in $fonts)
    {
        # Check if font family is installed by looking for any font files matching the family name
        # Font families can have multiple files (Regular, Bold, Italic, etc.)
        # Use more precise matching: check if any font file name starts with the font family name
        # followed by a space or hyphen to avoid partial matches (e.g., "Source Code Pro" vs "Source Code Pro Condensed")
        $escaped = [regex]::Escape($font)
        $systemFonts = Get-ChildItem -Path (Join-Path $env:windir "fonts") -ErrorAction SilentlyContinue | Where-Object { $_.Name -match "^$escaped[ -]" }
        $userFonts = Get-ChildItem -Path (Join-Path $env:LOCALAPPDATA "Microsoft\Windows\fonts") -ErrorAction SilentlyContinue | Where-Object { $_.Name -match "^$escaped[ -]" }

        if ($systemFonts.Count -eq 0 -and $userFonts.Count -eq 0)
        {
            if (-not $act)
            {
                $act = $true
                Write-Output ":: Installing Fonts"
            }

            if ($DryRun)
            {
                Write-Output "DRY-RUN: Would install font: $font"
            }
            else
            {
                Write-Verbose "Installing font: $font"
                $script = Join-Path (Join-Path $root "extern\fonts") install.ps1
                try
                {
                    & $script "$font"
                    if ($LASTEXITCODE -ne 0)
                    {
                        Write-Warning "Font installation script exited with code $LASTEXITCODE for font: $font"
                    }
                }
                catch
                {
                    Write-Warning "Failed to install font $font`: $_"
                }
            }
        }
        else
        {
            Write-Verbose "Skipping font $font`: already installed"
        }
    }
}
Export-ModuleMember -Function Install-Fonts