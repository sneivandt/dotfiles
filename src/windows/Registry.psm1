#Requires -PSEdition Desktop
#Requires -RunAsAdministrator

function Sync-Registry
{
    <#
    .SYNOPSIS
        Sync registry settings
    .DESCRIPTION
        Applies registry settings from configuration file.
        Format: Sections are registry paths, entries are name = value
    .PARAMETER root
        Repository root directory
    .PARAMETER DryRun
        When specified, logs actions that would be taken without making modifications
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root,

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    $configFile = Join-Path $root "conf\registry.ini"

    if (-not (Test-Path $configFile))
    {
        Write-Verbose "Skipping registry: no registry.ini found"
        return
    }

    # Use script scope for $act so Set-RegistryValue helper can modify it
    $script:act = $false

    # Read registry configuration from .ini file
    # Format: Sections are registry paths, entries are name = value
    $content = Get-Content $configFile
    $currentPath = $null
    $registryEntries = @()
    $lineNum = 0

    foreach ($line in $content)
    {
        $lineNum++
        $line = $line.Trim()

        # Skip empty lines and comments
        if ($line.Length -eq 0 -or $line -match '^\s*#')
        {
            continue
        }

        # Check for section header (registry path)
        if ($line -match '^\[(.+)\]$')
        {
            $currentPath = $matches[1]
            continue
        }

        # Parse key=value format
        if ($currentPath -and $line -match '^(.+?)\s*=\s*(.+)$')
        {
            $registryEntries += [PSCustomObject]@{
                Path = $currentPath
                Name = $matches[1].Trim()
                Value = $matches[2].Trim()
            }
        }
        elseif ($currentPath)
        {
            # Warn about malformed entries (not empty, not comment, not section, not key=value)
            Write-Warning "Line $lineNum`: Skipping malformed registry entry in [$currentPath]: $line (expected format: name = value)"
        }
    }

    foreach ($entry in $registryEntries)
    {
        $v = $entry.Value

        # Convert color table entries from hex to DWORD
        if ($entry.Name -like "ColorTable*" -and $v -match '^[0-9a-fA-F]{6}$')
        {
            $v = Convert-ConsoleColor "#$v"
        }

        Set-RegistryValue -Path $entry.Path -Name $entry.Name -Value $v -DryRun:$DryRun
    }
}
Export-ModuleMember -Function Sync-Registry

function Convert-ConsoleColor
{
    [OutputType([System.Int32])]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $rgb
    )

    if ($rgb -notmatch "^#[\da-f]{6}$")
    {
        Write-Error "Invalid color '$rgb' should be in RGB hex format, e.g. #000000"
        Return
    }

    $num = [Convert]::ToInt32($rgb.substring(1, 6), 16)
    $bytes = [BitConverter]::GetBytes($num)

    [Array]::Reverse($bytes, 0, 3)

    return [BitConverter]::ToInt32($bytes, 0)
}

function Set-RegistryValue
{
    [CmdletBinding(SupportsShouldProcess = $true)]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $path,
        [Parameter(Mandatory = $true)]
        [string]
        $name,
        [Parameter(Mandatory = $true)]
        [string]
        $value,
        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    # Treat -WhatIf like -DryRun for consistency
    $isDryRun = $DryRun -or $WhatIfPreference

    if (-not (Test-Path $path))
    {
        if (-not $script:act)
        {
            $script:act = $true
            Write-Output ":: Updating Registry"
        }

        if ($isDryRun)
        {
            Write-Output "DRY-RUN: Would create registry path: $path"
        }
        elseif ($PSCmdlet.ShouldProcess($path, "Create registry path"))
        {
            Write-Verbose "Creating registry path: $path"
            New-Item -Path $path -Type Folder | Out-Null
        }
    }

    # Get current value with type-aware comparison
    $currentValue = $null
    try
    {
        $currentValue = Get-ItemPropertyValue -Path $path -Name $name -ErrorAction SilentlyContinue
    }
    catch
    {
        # Value doesn't exist
        Write-Verbose "Registry value $name does not exist in $path"
    }

    $expandedValue = [Environment]::ExpandEnvironmentVariables($value)
    $needsUpdate = $false

    if ($null -eq $currentValue)
    {
        $needsUpdate = $true
    }
    elseif ($currentValue -is [int] -and $expandedValue -match '^-?\d+$')
    {
        # Both numeric - compare as integers
        $needsUpdate = ($currentValue -ne [int]$expandedValue)
    }
    elseif ($currentValue -is [int] -or $expandedValue -match '^0x[0-9a-fA-F]+$')
    {
        # Handle hex values - convert for comparison
        $currentInt = if ($currentValue -is [int]) { $currentValue } else { [Convert]::ToInt32($currentValue, 16) }
        $expandedInt = if ($expandedValue -match '^0x') { [Convert]::ToInt32($expandedValue, 16) } else { [int]$expandedValue }
        $needsUpdate = ($currentInt -ne $expandedInt)
    }
    else
    {
        # String comparison
        $needsUpdate = ($currentValue -ne $expandedValue)
    }

    if ($needsUpdate)
    {
        if (-not $script:act)
        {
            $script:act = $true
            Write-Output ":: Updating Registry"
        }

        if ($isDryRun)
        {
            Write-Output "DRY-RUN: Would set registry value: $path $name = $value"
        }
        elseif ($PSCmdlet.ShouldProcess("$path\$name", "Set registry value to $value"))
        {
            Write-Verbose "Setting registry value: $path $name = $value"
            Set-ItemProperty -Path $path -Name $name -Value $value
        }
    }
    else
    {
        Write-Verbose "Skipping registry value $name in $path`: already correct"
    }
}