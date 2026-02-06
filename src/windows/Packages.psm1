<#
.SYNOPSIS
    Windows package management for dotfiles
.DESCRIPTION
    Installs packages on Windows using winget (Windows Package Manager).
    Reads package definitions from conf/packages.ini and installs missing
    packages. Supports idempotent installations (skips already installed).

    Uses winget as the primary package manager because:
    - Built into Windows 11 and modern Windows 10
    - Official Microsoft package manager
    - No separate installation required
    - Wide package repository coverage

    Requires Windows Package Manager (winget) to be installed.
.NOTES
    Admin: Not required for most packages (winget handles elevation when needed)
#>

function Test-WingetInstalled
{
    <#
    .SYNOPSIS
        Check if winget is installed and available
    .DESCRIPTION
        Verifies that the winget command is available in the system.
    .OUTPUTS
        Boolean indicating whether winget is available
    #>
    [OutputType([System.Boolean])]
    [CmdletBinding()]
    param ()

    try
    {
        $null = Get-Command winget -ErrorAction Stop
        return $true
    }
    catch
    {
        return $false
    }
}

function Test-PackageInstalled
{
    <#
    .SYNOPSIS
        Check if a package is already installed via winget
    .DESCRIPTION
        Uses winget list to check if a package is installed.
        Quietly checks installation status without verbose output.
    .PARAMETER PackageId
        The winget package ID to check
    .OUTPUTS
        Boolean indicating whether the package is installed
    #>
    [OutputType([System.Boolean])]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $PackageId
    )

    try
    {
        # Use winget list to check if package is installed
        # Redirect output to avoid noise, just check exit code
        $output = winget list --id $PackageId --exact 2>&1
        if ($LASTEXITCODE -eq 0)
        {
            # Check if output contains the package (not just "No installed package found")
            $outputStr = $output | Out-String
            if ($outputStr -match [regex]::Escape($PackageId))
            {
                return $true
            }
        }
        return $false
    }
    catch
    {
        return $false
    }
}

function Install-Packages
{
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Function installs multiple packages, plural is appropriate')]
    <#
    .SYNOPSIS
        Install missing packages from conf/packages.ini
    .DESCRIPTION
        Reads package definitions from conf/packages.ini for sections matching
        the current profile. Installs missing packages using winget.

        Only processes packages from sections not excluded by the profile.
        Skips already-installed packages for idempotency.

        Configuration format in packages.ini:
          [windows]
          Microsoft.PowerShell
          Git.Git
          Microsoft.VisualStudioCode

        Uses winget package IDs (case-sensitive).
    .PARAMETER Root
        Repository root directory
    .PARAMETER ExcludedCategories
        Comma-separated list of categories to exclude from processing
    .PARAMETER DryRun
        When specified, logs actions without installing packages
    .EXAMPLE
        Install-Packages -Root $PSScriptRoot -ExcludedCategories "arch,desktop" -DryRun
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Root,

        [Parameter(Mandatory = $false)]
        [string]
        $ExcludedCategories = "",

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    # Check if winget is available
    if (-not (Test-WingetInstalled))
    {
        Write-Verbose "Skipping package installation: winget not installed"
        Write-Verbose "Install winget from: https://aka.ms/getwinget"
        return
    }

    # Check if packages.ini exists
    $configFile = Join-Path $Root "conf\packages.ini"
    if (-not (Test-Path $configFile))
    {
        Write-Verbose "Skipping packages: no packages.ini found"
        return
    }

    Write-Verbose "Processing packages from: conf/packages.ini"

    # Get all sections from packages.ini
    $sections = Get-Content $configFile |
        Where-Object { $_ -match '^\[.+\]$' } |
        ForEach-Object { $_ -replace '^\[|\]$', '' }

    # Track if we've printed the stage header
    $act = $false

    # Collect packages to install
    $packagesToInstall = @()

    foreach ($section in $sections)
    {
        # Check if this section should be included
        if (-not (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $ExcludedCategories))
        {
            Write-Verbose "Skipping packages section [$section]: profile not included"
            continue
        }

        # Read packages from this section
        $packages = Read-IniSection -FilePath $configFile -SectionName $section

        foreach ($package in $packages)
        {
            if ([string]::IsNullOrWhiteSpace($package))
            {
                continue
            }

            # Check if package is already installed
            if (Test-PackageInstalled -PackageId $package)
            {
                Write-Verbose "Skipping package $package`: already installed"
                continue
            }

            # Add to install list
            $packagesToInstall += $package
        }
    }

    # Install missing packages
    if ($packagesToInstall.Count -gt 0)
    {
        # Print stage header only once when we have work to do
        if (-not $act)
        {
            $act = $true
            Write-Output ":: Installing packages"
        }

        foreach ($package in $packagesToInstall)
        {
            if ($DryRun)
            {
                Write-Output "DRY-RUN: Would install package: $package"
            }
            else
            {
                Write-Verbose "Installing package: $package"
                # Install package with winget
                # --source winget: use winget repository (avoid msstore errors)
                # --silent: non-interactive installation
                # --accept-package-agreements: auto-accept package licenses
                # --accept-source-agreements: auto-accept source agreements
                winget install --id $package --exact --source winget --silent --accept-package-agreements --accept-source-agreements
                if ($LASTEXITCODE -ne 0)
                {
                    Write-Warning "Failed to install package: $package (exit code: $LASTEXITCODE)"
                }
            }
        }
    }
    else
    {
        Write-Verbose "No packages to install: all packages already present"
    }
}

# Export only the public function
Export-ModuleMember -Function Install-Packages
