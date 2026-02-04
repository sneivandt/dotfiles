<#
.SYNOPSIS
    Windows registry management for dotfiles
.DESCRIPTION
    Applies registry settings from configuration file using .NET registry APIs
    for PowerShell Core compatibility. Reads from conf/registry.ini where
    sections are registry paths and entries are name = value pairs.

    Note: Registry modification requires administrator privileges. The module
    supports dry-run mode which does not require elevation.
#>

function Get-RegistryHiveAndKey
{
    <#
    .SYNOPSIS
        Parse registry path into hive and subkey
    .DESCRIPTION
        Converts PowerShell registry path format (HKCU:\Software\Example)
        into .NET registry hive object and subkey path for PowerShell Core compatibility.
    .OUTPUTS
        PSCustomObject with Hive (RegistryKey) and SubKey (string) properties
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Path
    )

    # Parse registry path format: HIVE:\SubKey
    if ($Path -match '^(HKEY_CURRENT_USER|HKCU)[:\\](.*)$')
    {
        return [PSCustomObject]@{
            Hive = [Microsoft.Win32.Registry]::CurrentUser
            SubKey = $matches[2]
        }
    }
    elseif ($Path -match '^(HKEY_LOCAL_MACHINE|HKLM)[:\\](.*)$')
    {
        return [PSCustomObject]@{
            Hive = [Microsoft.Win32.Registry]::LocalMachine
            SubKey = $matches[2]
        }
    }
    elseif ($Path -match '^(HKEY_CLASSES_ROOT|HKCR)[:\\](.*)$')
    {
        return [PSCustomObject]@{
            Hive = [Microsoft.Win32.Registry]::ClassesRoot
            SubKey = $matches[2]
        }
    }
    elseif ($Path -match '^(HKEY_USERS|HKU)[:\\](.*)$')
    {
        return [PSCustomObject]@{
            Hive = [Microsoft.Win32.Registry]::Users
            SubKey = $matches[2]
        }
    }
    else
    {
        throw "Unsupported registry path format: $Path"
    }
}

function Test-RegistryPath
{
    <#
    .SYNOPSIS
        Test if registry path exists using .NET APIs
    .DESCRIPTION
        PowerShell Core compatible registry path existence check.
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Path
    )

    $parsed = Get-RegistryHiveAndKey -Path $Path
    $key = $parsed.Hive.OpenSubKey($parsed.SubKey, $false)
    $exists = $null -ne $key
    if ($key) { $key.Close() }
    return $exists
}

function New-RegistryPath
{
    <#
    .SYNOPSIS
        Create registry path using .NET APIs
    .DESCRIPTION
        PowerShell Core compatible registry path creation.
        This is an internal helper function. ShouldProcess is handled by calling function.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '',
        Justification='Internal helper function. ShouldProcess is handled by calling function Set-RegistryValue.')]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Path
    )

    $parsed = Get-RegistryHiveAndKey -Path $Path
    $key = $parsed.Hive.CreateSubKey($parsed.SubKey)
    if ($key) { $key.Close() }
}

function Get-RegistryValue
{
    <#
    .SYNOPSIS
        Get registry value using .NET APIs
    .DESCRIPTION
        PowerShell Core compatible registry value retrieval.
    .OUTPUTS
        Registry value or $null if not found
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Path,

        [Parameter(Mandatory = $true)]
        [string]
        $Name
    )

    $parsed = Get-RegistryHiveAndKey -Path $Path
    $key = $parsed.Hive.OpenSubKey($parsed.SubKey, $false)
    if (-not $key)
    {
        return $null
    }

    try
    {
        return $key.GetValue($Name, $null)
    }
    finally
    {
        $key.Close()
    }
}

function Set-RegistryKeyValue
{
    <#
    .SYNOPSIS
        Set registry value using .NET APIs
    .DESCRIPTION
        PowerShell Core compatible registry value setting.
        This is an internal helper function. ShouldProcess is handled by calling function.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '',
        Justification='Internal helper function. ShouldProcess is handled by calling function Set-RegistryValue.')]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Path,

        [Parameter(Mandatory = $true)]
        [string]
        $Name,

        [Parameter(Mandatory = $true)]
        $Value
    )

    $parsed = Get-RegistryHiveAndKey -Path $Path
    $key = $parsed.Hive.OpenSubKey($parsed.SubKey, $true)
    if (-not $key)
    {
        throw "Registry path does not exist: $Path"
    }

    try
    {
        # Determine registry value type
        $valueKind = if ($Value -is [int])
        {
            [Microsoft.Win32.RegistryValueKind]::DWord
        }
        else
        {
            [Microsoft.Win32.RegistryValueKind]::String
        }

        $key.SetValue($Name, $Value, $valueKind)
    }
    finally
    {
        $key.Close()
    }
}

function Sync-Registry
{
    <#
    .SYNOPSIS
        Sync registry settings
    .DESCRIPTION
        Applies registry settings from configuration file.
        Format: Sections are registry paths, entries are name = value
        Uses .NET registry APIs for PowerShell Core compatibility.
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

    if (-not (Test-RegistryPath -Path $path))
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
            New-RegistryPath -Path $path
        }
    }

    # Get current value with type-aware comparison
    $currentValue = $null
    $valueExists = $false

    # In dry-run mode, path might not exist yet
    if (-not (Test-RegistryPath -Path $path))
    {
        Write-Verbose "Registry path $path does not exist (will be created)"
        $valueExists = $false
    }
    else
    {
        try
        {
            $currentValue = Get-RegistryValue -Path $path -Name $name
            $valueExists = $null -ne $currentValue
            if (-not $valueExists)
            {
                Write-Verbose "Registry value $name does not exist in $path"
            }
        }
        catch
        {
            # Unexpected error (permission denied, etc.)
            Write-Warning "Failed to read registry value $name from $path`: $_"
            return
        }
    }

    $expandedValue = [Environment]::ExpandEnvironmentVariables($value)
    $needsUpdate = $false

    if (-not $valueExists)
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
            # Convert numeric strings to integers for proper registry type
            $finalValue = if ($value -match '^-?\d+$') { [int]$value } else { $value }
            Set-RegistryKeyValue -Path $path -Name $name -Value $finalValue
        }
    }
    else
    {
        Write-Verbose "Skipping registry value $name in $path`: already correct"
    }
}