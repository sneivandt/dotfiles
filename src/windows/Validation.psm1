#Requires -PSEdition Desktop

<#
.SYNOPSIS
    Validation utilities for Windows dotfiles
.DESCRIPTION
    Provides pre-flight validation checks to ensure the system is ready
    for dotfiles installation and to report what will be installed.
#>

function Show-InstallationPreview
{
    <#
    .SYNOPSIS
        Show a preview of what will be installed
    .DESCRIPTION
        Analyzes the configuration and reports what symlinks will be created,
        what fonts will be installed, what VS Code extensions will be added,
        and what registry settings will be modified.
    .PARAMETER root
        Repository root directory
    .PARAMETER excludedCategories
        Comma-separated list of categories to exclude
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root,

        [Parameter(Mandatory = $false)]
        [string]
        $excludedCategories = ""
    )

    Write-Output ""
    Write-Output "=== Installation Preview ==="
    Write-Output ""

    # Preview symlinks
    $symlinkConfig = Join-Path $root "conf\symlinks.ini"
    if (Test-Path $symlinkConfig)
    {
        $content = Get-Content $symlinkConfig
        $sections = @()
        foreach ($line in $content)
        {
            $line = $line.Trim()
            if ($line -match '^\[(.+)\]$')
            {
                $sections += $matches[1]
            }
        }

        $symlinkCount = 0
        $missingCount = 0
        foreach ($section in $sections)
        {
            if (-not (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excludedCategories))
            {
                continue
            }

            $links = Read-IniSection -FilePath $symlinkConfig -SectionName $section
            foreach ($link in $links)
            {
                $sourcePath = Join-Path $root "symlinks\$link"
                if (Test-Path $sourcePath)
                {
                    $symlinkCount++
                }
                else
                {
                    $missingCount++
                    Write-Verbose "Source file excluded by sparse checkout: $link"
                }
            }
        }

        Write-Output "Symlinks: $symlinkCount will be created"
        if ($missingCount -gt 0)
        {
            Write-Output "          $missingCount will be skipped (excluded by sparse checkout)"
        }
    }

    # Preview fonts
    $fontConfig = Join-Path $root "conf\fonts.ini"
    if (Test-Path $fontConfig)
    {
        $fonts = Read-IniSection -FilePath $fontConfig -SectionName "fonts"
        $fontCount = 0
        $installedCount = 0

        foreach ($font in $fonts)
        {
            $escaped = [regex]::Escape($font)
            $systemFonts = Get-ChildItem -Path (Join-Path $env:windir "fonts") -ErrorAction SilentlyContinue | Where-Object { $_.Name -match "^$escaped[ -]" }
            $userFonts = Get-ChildItem -Path (Join-Path $env:LOCALAPPDATA "Microsoft\Windows\fonts") -ErrorAction SilentlyContinue | Where-Object { $_.Name -match "^$escaped[ -]" }

            if ($systemFonts.Count -eq 0 -and $userFonts.Count -eq 0)
            {
                $fontCount++
            }
            else
            {
                $installedCount++
            }
        }

        Write-Output "Fonts:    $fontCount will be installed"
        if ($installedCount -gt 0)
        {
            Write-Output "          $installedCount already installed"
        }
    }

    # Preview VS Code extensions
    $vscodeConfig = Join-Path $root "conf\vscode-extensions.ini"
    if (Test-Path $vscodeConfig)
    {
        # Collect extensions from all applicable sections
        $allExtensions = @()
        $content = Get-Content $vscodeConfig
        $sections = @()
        foreach ($line in $content)
        {
            $line = $line.Trim()
            if ($line -match '^\[(.+)\]$')
            {
                $sections += $matches[1]
            }
        }

        foreach ($section in $sections)
        {
            if (-not (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excludedCategories))
            {
                continue
            }
            $sectionExtensions = Read-IniSection -FilePath $vscodeConfig -SectionName $section
            $allExtensions += $sectionExtensions
        }

        $allExtensions = $allExtensions | Select-Object -Unique
        $totalToInstall = 0

        foreach ($code in @('code', 'code-insiders'))
        {
            if (-not (Get-Command $code -ErrorAction SilentlyContinue))
            {
                continue
            }

            $installed = & $code --list-extensions
            $toInstall = 0

            foreach ($extension in $allExtensions)
            {
                if ($installed -notcontains $extension)
                {
                    $toInstall++
                }
            }

            if ($toInstall -gt 0)
            {
                Write-Output "VS Code Extensions ($code): $toInstall will be installed"
                $totalToInstall += $toInstall
            }
        }

        if ($totalToInstall -eq 0)
        {
            Write-Output "VS Code Extensions: All already installed or VS Code not found"
        }
    }

    # Preview registry settings
    $registryConfig = Join-Path $root "conf\registry.ini"
    if (Test-Path $registryConfig)
    {
        $content = Get-Content $registryConfig
        $entryCount = 0
        foreach ($line in $content)
        {
            $line = $line.Trim()
            if ($line.Length -gt 0 -and $line -notmatch '^\s*#' -and $line -notmatch '^\[' -and $line -match '=')
            {
                $entryCount++
            }
        }
        Write-Output "Registry: $entryCount settings will be checked/updated"
    }

    # Preview git submodules
    $submodulesConfig = Join-Path $root "conf\submodules.ini"
    if (Test-Path $submodulesConfig)
    {
        Push-Location $root
        $status = git submodule status 2>&1
        Pop-Location

        if ($status -match "^[+\-]")
        {
            $moduleCount = ($status | Measure-Object).Count
            Write-Output "Git Submodules: $moduleCount will be initialized/updated"
        }
        else
        {
            Write-Output "Git Submodules: All up to date"
        }
    }

    Write-Output ""
}

Export-ModuleMember -Function Show-InstallationPreview
