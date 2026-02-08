<#
.SYNOPSIS
    Logging and telemetry utilities for Windows dotfiles
.DESCRIPTION
    Provides logging functions, counter tracking, and summary statistics
    for the dotfiles installation process on Windows. Supports persistent
    log files and operation counting for summary reporting.
.NOTES
    All logging output is mirrored to a persistent log file for troubleshooting.
#>

# Module-level variables for log file location
$script:LogDir = Join-Path $env:LOCALAPPDATA "dotfiles"
$script:LogFile = Join-Path $script:LogDir "install.log"
$script:CounterDir = Join-Path $script:LogDir "counters"

function Initialize-Logging
{
    <#
    .SYNOPSIS
        Initialize the logging system
    .DESCRIPTION
        Creates log directory and file, resets counters. Should be called
        once at the start of install/uninstall operations.
    .PARAMETER Profile
        Profile name to include in log header
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $false)]
        [string]
        $Profile = "windows"
    )

    # Create log directory if it doesn't exist
    if (-not (Test-Path $script:LogDir))
    {
        New-Item -Path $script:LogDir -ItemType Directory -Force | Out-Null
    }

    if (-not (Test-Path $script:CounterDir))
    {
        New-Item -Path $script:CounterDir -ItemType Directory -Force | Out-Null
    }

    # Initialize log file with timestamp
    $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    @"
==========================================
Dotfiles $timestamp
Profile: $Profile
==========================================
"@ | Out-File -FilePath $script:LogFile -Encoding utf8

    # Reset all counters
    if (Test-Path $script:CounterDir)
    {
        Get-ChildItem -Path $script:CounterDir -File | Remove-Item -Force
    }
}

function Write-LogMessage
{
    <#
    .SYNOPSIS
        Internal function to write a message to the log file
    .DESCRIPTION
        Strips ANSI color codes and writes the message to the persistent log file.
    .PARAMETER Message
        Message to log
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Message
    )

    if (Test-Path $script:LogFile)
    {
        # Strip ANSI color codes (basic pattern - PowerShell doesn't typically use them in default output)
        $cleanMessage = $Message -replace '\x1b\[[0-9;]*m', ''
        $cleanMessage | Out-File -FilePath $script:LogFile -Append -Encoding utf8
    }
}

function Write-Progress
{
    <#
    .SYNOPSIS
        Write a progress message at the default log level
    .DESCRIPTION
        Provides feedback about what is being checked/processed without
        being as detailed as verbose mode.
    .PARAMETER Message
        Progress description (e.g., "Checking packages", "Installing symlinks")
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Message
    )

    $output = "   $Message"
    Write-Output $output
    Write-LogMessage -Message $output
}

function Write-Stage
{
    <#
    .SYNOPSIS
        Write a stage heading
    .DESCRIPTION
        Prints a stage heading with :: prefix for visual grouping.
    .PARAMETER Message
        Stage description
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Message
    )

    $output = ":: $Message"
    Write-Output $output
    Write-LogMessage -Message $output
}

function Write-DryRunMessage
{
    <#
    .SYNOPSIS
        Write a dry-run message
    .DESCRIPTION
        Prints a message indicating what would happen in dry-run mode.
    .PARAMETER Message
        Action description
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Message
    )

    $output = "DRY-RUN: $Message"
    Write-Output $output
    Write-LogMessage -Message $output
}

function Increment-Counter
{
    <#
    .SYNOPSIS
        Increment a named counter for summary statistics
    .DESCRIPTION
        Increments a counter by 1 for tracking operations performed.
    .PARAMETER CounterName
        Counter name (e.g., "packages_installed", "symlinks_created")
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $CounterName
    )

    $counterFile = Join-Path $script:CounterDir $CounterName

    # Read current value (default to 0)
    $current = 0
    if (Test-Path $counterFile)
    {
        $current = [int](Get-Content $counterFile -Raw)
    }

    # Increment and write back
    ($current + 1) | Out-File -FilePath $counterFile -Encoding utf8
}

function Get-Counter
{
    <#
    .SYNOPSIS
        Get the current value of a named counter
    .DESCRIPTION
        Returns the counter value, or 0 if the counter doesn't exist.
    .PARAMETER CounterName
        Counter name
    .OUTPUTS
        [int] Counter value
    #>
    [OutputType([int])]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $CounterName
    )

    $counterFile = Join-Path $script:CounterDir $CounterName

    if (Test-Path $counterFile)
    {
        return [int](Get-Content $counterFile -Raw)
    }
    else
    {
        return 0
    }
}

function Write-InstallationSummary
{
    <#
    .SYNOPSIS
        Print a summary of all operations performed
    .DESCRIPTION
        Displays counters for packages, symlinks, extensions, etc.
        Should be called at the end of install/uninstall operations.
    #>
    [CmdletBinding()]
    param ()

    Write-Stage -Message "Installation Summary"

    # Get counter values
    $packagesInstalled = Get-Counter -CounterName "packages_installed"
    $symlinksCreated = Get-Counter -CounterName "symlinks_created"
    $symlinksRemoved = Get-Counter -CounterName "symlinks_removed"
    $vscodeExtensionsInstalled = Get-Counter -CounterName "vscode_extensions_installed"
    $registryKeysSet = Get-Counter -CounterName "registry_keys_set"

    # Build summary
    $hasChanges = $false

    if ($packagesInstalled -gt 0)
    {
        Write-Output "   Packages installed: $packagesInstalled"
        $hasChanges = $true
    }

    if ($symlinksCreated -gt 0)
    {
        Write-Output "   Symlinks created: $symlinksCreated"
        $hasChanges = $true
    }

    if ($symlinksRemoved -gt 0)
    {
        Write-Output "   Symlinks removed: $symlinksRemoved"
        $hasChanges = $true
    }

    if ($vscodeExtensionsInstalled -gt 0)
    {
        Write-Output "   VS Code extensions installed: $vscodeExtensionsInstalled"
        $hasChanges = $true
    }

    if ($registryKeysSet -gt 0)
    {
        Write-Output "   Registry keys set: $registryKeysSet"
        $hasChanges = $true
    }

    if (-not $hasChanges)
    {
        Write-Output "   No changes made (all components already configured)"
    }

    # Log file location
    if (Test-Path $script:LogFile)
    {
        Write-Output "   Log file: $script:LogFile"
    }

    # Write summary to log file
    Write-LogMessage -Message ""
    Write-LogMessage -Message "=========================================="
    Write-LogMessage -Message "Installation Summary"
    Write-LogMessage -Message "=========================================="

    if ($hasChanges)
    {
        if ($packagesInstalled -gt 0)
        {
            Write-LogMessage -Message "   Packages installed: $packagesInstalled"
        }
        if ($symlinksCreated -gt 0)
        {
            Write-LogMessage -Message "   Symlinks created: $symlinksCreated"
        }
        if ($symlinksRemoved -gt 0)
        {
            Write-LogMessage -Message "   Symlinks removed: $symlinksRemoved"
        }
        if ($vscodeExtensionsInstalled -gt 0)
        {
            Write-LogMessage -Message "   VS Code extensions installed: $vscodeExtensionsInstalled"
        }
        if ($registryKeysSet -gt 0)
        {
            Write-LogMessage -Message "   Registry keys set: $registryKeysSet"
        }
    }
    else
    {
        Write-LogMessage -Message "   No changes made (all components already configured)"
    }
}

# Export only the public functions
Export-ModuleMember -Function @(
    'Initialize-Logging',
    'Write-Progress',
    'Write-Stage',
    'Write-DryRunMessage',
    'Increment-Counter',
    'Get-Counter',
    'Write-InstallationSummary'
)
