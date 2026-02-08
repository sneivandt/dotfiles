<#
.SYNOPSIS
    Windows registry management for dotfiles
.DESCRIPTION
    Applies registry settings from configuration file using .NET registry APIs
    compatible with both PowerShell Core and Windows PowerShell. Reads from
    conf/registry.ini where sections are registry paths and entries are name = value pairs.
.NOTES
    Admin: Required for registry modification (not required in dry-run mode)
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
        Test if registry path exists
    .DESCRIPTION
        PowerShell Core compatible registry path existence check using native cmdlet.
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Path
    )

    return Test-Path -Path $Path -ErrorAction SilentlyContinue
}

function New-RegistryPath
{
    <#
    .SYNOPSIS
        Create registry path using PowerShell cmdlets
    .DESCRIPTION
        PowerShell Core compatible registry path creation.
        Creates parent paths recursively if needed.
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

    # Use PowerShell cmdlet instead of .NET API for better compatibility
    # New-Item will create parent paths if -Force is used
    try
    {
        $null = New-Item -Path $Path -Force -ErrorAction Stop
    }
    catch
    {
        throw "Failed to create registry path: $Path - $_"
    }
}

function Get-RegistryValue
{
    <#
    .SYNOPSIS
        Get registry value
    .DESCRIPTION
        PowerShell Core compatible registry value retrieval using native cmdlet.
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

    try
    {
        $item = Get-ItemProperty -Path $Path -Name $Name -ErrorAction Stop
        return $item.$Name
    }
    catch
    {
        return $null
    }
}

function Set-RegistryKeyValue
{
    <#
    .SYNOPSIS
        Set registry value
    .DESCRIPTION
        PowerShell Core compatible registry value setting using native cmdlet.
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

    # Determine property type for Set-ItemProperty
    $propertyType = if ($Value -is [int])
    {
        'DWord'
    }
    else
    {
        'String'
    }

    try
    {
        Set-ItemProperty -Path $Path -Name $Name -Value $Value -Type $propertyType -ErrorAction Stop
    }
    catch
    {
        throw "Failed to set registry value: $Path\$Name - $_"
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
    Write-Verbose "Reading registry configuration from: conf/registry.ini"

    if (-not (Test-Path $configFile))
    {
        Write-Verbose "Skipping registry: no registry.ini found"
        return
    }

    Write-ProgressMessage -Message "Checking registry settings..."

    # Use script scope for $act so Set-RegistryValue helper can modify it
    $script:act = $false

    # Read registry configuration from .ini file
    # Format: Sections are registry paths, entries are name = value
    Write-Verbose "Parsing registry.ini file..."
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
            $valuePart = $matches[2].Trim()
            # Strip inline comments (everything after # character, including #)
            # Handle both "value # comment" and "value# comment" formats
            $commentIndex = $valuePart.IndexOf('#')
            if ($commentIndex -ge 0)
            {
                $valuePart = $valuePart.Substring(0, $commentIndex).Trim()
            }

            $registryEntries += [PSCustomObject]@{
                Path = $currentPath
                Name = $matches[1].Trim()
                Value = $valuePart
            }
        }
        elseif ($currentPath)
        {
            # Warn about malformed entries (not empty, not comment, not section, not key=value)
            Write-Warning "Line $lineNum`: Skipping malformed registry entry in [$currentPath]: $line (expected format: name = value)"
        }
    }

    Write-Verbose "Found $($registryEntries.Count) registry value(s) across $($registryEntries | Select-Object -ExpandProperty Path -Unique | Measure-Object | Select-Object -ExpandProperty Count) key(s)"

    # Group entries by registry path for batch reading
    # This significantly improves performance by reading all values from each key at once
    $entriesByPath = $registryEntries | Group-Object -Property Path

    foreach ($pathGroup in $entriesByPath)
    {
        $registryPath = $pathGroup.Name
        $entries = $pathGroup.Group
        Write-Verbose "Processing registry key: $registryPath ($($entries.Count) value(s))"

        # Batch read all current values for this registry path
        # This is much faster than individual Get-RegistryValue calls for each entry
        $currentValues = @{}
        if (Test-RegistryPath -Path $registryPath)
        {
            try
            {
                $keyProperties = Get-ItemProperty -Path $registryPath -ErrorAction Stop
                # Store all property values in hashtable for fast lookup
                # Skip PowerShell's built-in properties that Get-ItemProperty adds
                $psBuiltInProperties = @('PSPath', 'PSParentPath', 'PSChildName', 'PSProvider', 'PSDrive')
                foreach ($property in $keyProperties.PSObject.Properties)
                {
                    if ($property.Name -notin $psBuiltInProperties)
                    {
                        $currentValues[$property.Name] = $property.Value
                    }
                }
            }
            catch
            {
                Write-Warning "Failed to read registry key $registryPath`: $_"
            }
        }

        # Process each entry in this path
        foreach ($entry in $entries)
        {
            $v = $entry.Value

            # Convert color table entries from hex to DWORD
            if ($entry.Name -like "ColorTable*" -and $v -match '^[0-9a-fA-F]{6}$')
            {
                $v = Convert-ConsoleColor "#$v"
            }

            # Pass current values to avoid redundant registry reads
            Set-RegistryValue -Path $entry.Path -Name $entry.Name -Value $v -CurrentValues $currentValues -DryRun:$DryRun
        }
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
        $Path,
        [Parameter(Mandatory = $true)]
        [string]
        $Name,
        [Parameter(Mandatory = $true)]
        [string]
        $Value,
        [Parameter(Mandatory = $false)]
        [hashtable]
        $CurrentValues,
        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    # Treat -WhatIf like -DryRun for consistency
    $isDryRun = $DryRun -or $WhatIfPreference

    if (-not (Test-RegistryPath -Path $Path))
    {
        if (-not $script:act)
        {
            $script:act = $true
            Write-Stage -Message "Updating Registry"
        }

        if ($isDryRun)
        {
            Write-DryRunMessage -Message "Would create registry path: $Path"
        }
        elseif ($PSCmdlet.ShouldProcess($Path, "Create registry path"))
        {
            Write-Verbose "Creating registry path: $Path"
            try
            {
                New-RegistryPath -Path $Path

                # Verify path was created
                if (-not (Test-RegistryPath -Path $Path))
                {
                    Write-Warning "Failed to create registry path: $Path (path does not exist after creation attempt)"
                    return
                }
            }
            catch
            {
                Write-Warning "Failed to create registry path: $Path - $_"
                return
            }
        }
    }

    # Get current value with type-aware comparison
    # If CurrentValues hashtable is provided (batch mode), use it for much faster lookup
    # Otherwise, fall back to individual registry read (legacy compatibility)
    $currentValue = $null
    $valueExists = $false

    # In dry-run mode, path might not exist yet
    if (-not (Test-RegistryPath -Path $Path))
    {
        Write-Verbose "Registry path $Path does not exist (will be created)"
        $valueExists = $false
    }
    elseif ($null -ne $CurrentValues)
    {
        # Batch mode: Use pre-read values from hashtable (fast)
        $valueExists = $CurrentValues.ContainsKey($Name)
        if ($valueExists)
        {
            $currentValue = $CurrentValues[$Name]
        }
        else
        {
            Write-Verbose "Registry value $Name does not exist in $Path"
        }
    }
    else
    {
        # Legacy mode: Individual registry read (slow, for backward compatibility)
        try
        {
            $currentValue = Get-RegistryValue -Path $Path -Name $Name
            $valueExists = $null -ne $currentValue
            if (-not $valueExists)
            {
                Write-Verbose "Registry value $Name does not exist in $Path"
            }
        }
        catch
        {
            # Unexpected error (permission denied, etc.)
            Write-Warning "Failed to read registry value $Name from $Path`: $_"
            return
        }
    }

    # Don't expand environment variables for comparison - registry values can contain unexpanded vars
    $needsUpdate = $false

    if (-not $valueExists)
    {
        $needsUpdate = $true
    }
    elseif ($currentValue -is [int] -and $Value -match '^-?\d+$')
    {
        # Both numeric - compare as integers
        $needsUpdate = ($currentValue -ne [int]$Value)
    }
    elseif ($currentValue -is [int] -or $currentValue -is [uint32] -or $Value -match '^0x[0-9a-fA-F]+$' -or $currentValue -match '^0x[0-9a-fA-F]+$')
    {
        # Handle hex and numeric values - convert for comparison
        # Registry DWORD values can be Int32 (signed) or UInt32 (unsigned)
        # Need to handle both negative values and large positive values (like color codes)

        # Detect corrupted values (e.g., "0x00200078 # comment") and mark for update
        if ($currentValue -is [string] -and $currentValue.Contains('#'))
        {
            Write-Verbose "Detected corrupted registry value (contains comment): $Name = $currentValue"
            $needsUpdate = $true
        }
        else
        {
            try
            {
                # First, determine what the config value represents
                $valueInt = if ($Value -match '^0x([0-9a-fA-F]+)$')
                {
                    # Hex value - convert as unsigned (colors, etc.)
                    # Don't cast to int64 as it would lose unsigned data for large values
                    [Convert]::ToUInt64($matches[1], 16)
                }
                elseif ($Value -match '^-\d+$')
                {
                    # Negative decimal value
                    [int64]$Value
                }
                else
                {
                    # Positive decimal value
                    [int64]$Value
                }

                # Convert current registry value to match the expected type
                $currentInt = if ($currentValue -is [int])
                {
                    [int64]$currentValue
                }
                elseif ($currentValue -is [uint32])
                {
                    # If config value is negative and registry value is large, convert two's complement
                    if ($valueInt -lt 0 -and [uint32]$currentValue -ge 0x80000000)
                    {
                        # Convert two's complement: subtract 2^32 to get negative value
                        [int64]$currentValue - 0x100000000
                    }
                    else
                    {
                        # Positive value or color code - keep as unsigned
                        [int64]$currentValue
                    }
                }
                elseif ($currentValue -match '^0x([0-9a-fA-F]+)$')
                {
                    # Don't cast to int64 as it would lose unsigned data for large values
                    [Convert]::ToUInt64($matches[1], 16)
                }
                else
                {
                    [int64]$currentValue
                }

                $needsUpdate = ($currentInt -ne $valueInt)
            }
            catch
            {
                # Conversion failed - likely corrupted value, mark for update
                Write-Verbose "Failed to convert registry value for comparison (likely corrupted): $Name = $currentValue - $_"
                $needsUpdate = $true
            }
        }
    }
    else
    {
        # String comparison - compare literally without expanding environment variables
        $needsUpdate = ($currentValue -ne $Value)
    }

    if ($needsUpdate)
    {
        if (-not $script:act)
        {
            $script:act = $true
            Write-Stage -Message "Updating Registry"
        }

        if ($isDryRun)
        {
            Write-DryRunMessage -Message "Would set registry value: $Path $Name = $Value"
        }
        elseif ($PSCmdlet.ShouldProcess("$Path\$Name", "Set registry value to $Value"))
        {
            Write-Verbose "Setting registry value: $Path $Name = $Value"
            # Convert numeric strings and hex values to integers for proper registry type
            $finalValue = if ($Value -match '^0x([0-9a-fA-F]+)$')
            {
                # Use ToUInt32 for hex values to handle color codes correctly (prevents overflow for values > 0x7FFFFFFF)
                [Convert]::ToUInt32($matches[1], 16)
            }
            elseif ($Value -match '^-?\d+$')
            {
                [int]$Value
            }
            else
            {
                $Value
            }
            Set-RegistryKeyValue -Path $Path -Name $Name -Value $finalValue
            Increment-Counter -CounterName "registry_keys_set"
        }
    }
    else
    {
        Write-Verbose "Skipping registry value $Name in $Path`: already correct"
    }
}