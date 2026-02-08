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

function Get-InstalledPackages
{
    <#
    .SYNOPSIS
        Get list of all installed packages via winget
    .DESCRIPTION
        Retrieves all installed packages from winget in a single call.
        Much faster than checking packages individually.
    .OUTPUTS
        Array of package IDs that are currently installed
    #>
    # Plural name justified: function returns multiple packages as array
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Function returns multiple packages, plural is appropriate')]
    [OutputType([System.String[]])]
    [CmdletBinding()]
    param ()

    try
    {
        Write-Verbose "Retrieving list of all installed packages from winget..."

        # Use winget export to get JSON output which is much more reliable than parsing text output
        # Export to a temp file then read it
        $tempFile = [System.IO.Path]::GetTempFileName()
        try
        {
            # Export installed packages to JSON
            $null = winget export --output $tempFile --accept-source-agreements 2>&1

            if ($LASTEXITCODE -ne 0 -or -not (Test-Path $tempFile))
            {
                Write-Verbose "Failed to export package list (exit code: $LASTEXITCODE), falling back to list command"
                # Fallback: parse winget list output
                return Get-InstalledPackagesFallback
            }

            # Read and parse JSON
            $json = Get-Content $tempFile -Raw | ConvertFrom-Json
            $installedPackages = @()

            if ($json.Sources)
            {
                foreach ($source in $json.Sources)
                {
                    if ($source.Packages)
                    {
                        foreach ($package in $source.Packages)
                        {
                            if ($package.PackageIdentifier)
                            {
                                $installedPackages += $package.PackageIdentifier
                                Write-Verbose "  Found installed package: $($package.PackageIdentifier)"
                            }
                        }
                    }
                }
            }

            Write-Verbose "Found $($installedPackages.Count) installed package(s) total"
            return $installedPackages
        }
        finally
        {
            # Clean up temp file
            if (Test-Path $tempFile)
            {
                Remove-Item $tempFile -Force -ErrorAction SilentlyContinue
            }
        }
    }
    catch
    {
        Write-Verbose "Error retrieving installed packages: $_"
        return [string[]]@()
    }
}

function Get-InstalledPackagesFallback
{
    <#
    .SYNOPSIS
        Fallback method to get installed packages by parsing winget list output
    .DESCRIPTION
        Used when winget export fails. Less reliable but better than nothing.
    #>
    [OutputType([System.String[]])]
    [CmdletBinding()]
    param ()

    try
    {
        Write-Verbose "Using fallback method to parse winget list output..."
        $output = winget list --accept-source-agreements 2>&1

        if ($LASTEXITCODE -ne 0)
        {
            Write-Verbose "Fallback method failed (exit code: $LASTEXITCODE)"
            return [string[]]@()
        }

        $installedPackages = @()
        $stringOutput = ($output | Out-String) -split "`n"

        foreach ($line in $stringOutput)
        {
            # Match package IDs more strictly - must contain at least one dot and be reasonable length
            if ($line -match '\b([A-Za-z][A-Za-z0-9]{2,}\.[A-Za-z0-9][\w\.\-]{2,})\b')
            {
                $packageId = $matches[1]
                # Additional validation: reasonable package ID format
                if ($packageId.Length -le 100 -and ($packageId.Split('.').Count -ge 2))
                {
                    $installedPackages += $packageId
                    Write-Verbose "  Found installed package: $packageId"
                }
            }
        }

        Write-Verbose "Fallback found $($installedPackages.Count) installed package(s)"
        return $installedPackages
    }
    catch
    {
        Write-Verbose "Fallback method error: $_"
        return [string[]]@()
    }
}

function Install-Packages
{
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
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Function installs multiple packages, plural is appropriate')]
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

    Write-ProgressMessage -Message "Checking packages..."
    Write-Verbose "Processing packages from: conf/packages.ini"

    # Get all sections from packages.ini
    $sections = Get-Content $configFile |
        Where-Object { $_ -match '^\[.+\]$' } |
        ForEach-Object { $_ -replace '^\[|\]$', '' }

    Write-Verbose "Found $($sections.Count) section(s) in packages.ini: $($sections -join ', ')"

    # Track if we've printed the stage header
    $act = $false

    # Get all installed packages once (much faster than checking individually)
    $installedPackages = Get-InstalledPackages

    # Collect packages to install
    $packagesToInstall = @()

    foreach ($section in $sections)
    {
        Write-Verbose "Processing packages section: [$section]"

        # Check if this section should be included
        if (-not (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $ExcludedCategories))
        {
            Write-Verbose "Skipping packages section [$section]: profile not included"
            continue
        }

        # Read packages from this section
        $packages = Read-IniSection -FilePath $configFile -SectionName $section
        Write-Verbose "Found $($packages.Count) package(s) in section [$section]"

        foreach ($package in $packages)
        {
            if ([string]::IsNullOrWhiteSpace($package))
            {
                continue
            }

            Write-Verbose "Checking package: $package"

            # Check if package is already installed (using cached list, case-insensitive)
            if ($installedPackages -contains $package)
            {
                Write-Verbose "Skipping package $package`: already installed"
                continue
            }

            # Add to install list
            Write-Verbose "Package $package needs installation"
            $packagesToInstall += $package
        }
    }

    Write-Verbose "Total packages to install: $($packagesToInstall.Count)"

    # Install missing packages
    if ($packagesToInstall.Count -gt 0)
    {
        foreach ($package in $packagesToInstall)
        {
            if ($DryRun)
            {
                # Print stage header only once in dry-run mode
                if (-not $act)
                {
                    $act = $true
                    Write-Stage -Message "Installing packages"
                }
                Write-DryRunMessage -Message "Would install package: $package"
                Increment-Counter -CounterName "packages_installed"
            }
            else
            {
                # Print stage header before attempting installation
                if (-not $act)
                {
                    $act = $true
                    Write-Stage -Message "Installing packages"
                }

                Write-Verbose "Installing package: $package"
                Write-Output "Installing: $package"

                # Install package with winget
                # --source winget: use winget repository (avoid msstore errors)
                # --silent: non-interactive installation
                # --accept-package-agreements: auto-accept package licenses
                # --accept-source-agreements: auto-accept source agreements
                $output = winget install --id $package --exact --source winget --silent --accept-package-agreements --accept-source-agreements 2>&1
                $exitCode = $LASTEXITCODE

                # Handle different exit codes
                if ($exitCode -eq 0)
                {
                    # Success
                    Write-Verbose "Successfully installed: $package"
                    # Show output for successful installations
                    $output | Out-String | Write-Output
                    Increment-Counter -CounterName "packages_installed"
                }
                # WinGet exit code 0x8A150055 (-1978335189 in signed int32) indicates package is already installed
                # This is a known WinGet constant: APPINSTALLER_CLI_ERROR_PACKAGE_ALREADY_INSTALLED
                elseif ($exitCode -eq -1978335189)
                {
                    # Package already installed (exit code -1978335189)
                    Write-Verbose "Package $package already installed (winget reported as installed)"
                    Write-Output "Already installed: $package"
                }
                else
                {
                    # Other errors - show warning and output
                    Write-Warning "Failed to install package: $package (exit code: $exitCode)"
                    $output | Out-String | Write-Output
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
