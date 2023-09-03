#Requires -PSEdition Desktop
#Requires -RunAsAdministrator

function Sync-Registry
{
    <#
    .SYNOPSIS
        Sync registry
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseShouldProcessForStateChangingFunctions", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root
    )

    $script:act = $false

    $registryConfig = Get-Content $root\env\win\registry.json | ConvertFrom-Json
    $shellConfig = Get-Content $root\env\win\registry-shell.json | ConvertFrom-Json

    $consoleKeys = @(
        "HKCU:\Console\%SystemRoot%_System32_bash.exe",
        "HKCU:\Console\%SystemRoot%_System32_WindowsPowerShell_v1.0_powershell.exe",
        "HKCU:\Console\%SystemRoot%_SysWOW64_WindowsPowerShell_v1.0_powershell.exe",
        "HKCU:\Console\Windows PowerShell (x86)",
        "HKCU:\Console\Windows PowerShell",
        "HKCU:\Console"
    )

    foreach ($entry in $registryConfig)
    {
        Set-RegistryValue -Path $entry.Path -Name $entry.Name -Value $entry.Value
    }

    foreach ($consoleKey in $consoleKeys)
    {
        foreach ($entry in $shellConfig)
        {
            $v = $entry.Value

            if ($entry.Name -like "ColorTable*")
            {
                $v = Convert-ConsoleColor "#$v"
            }

            Set-RegistryValue -Path $consoleKey -Name $entry.Name -Value $v
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
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseShouldProcessForStateChangingFunctions", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $path,
        [Parameter(Mandatory = $true)]
        [string]
        $name,
        [Parameter(Mandatory = $true)]
        [string]
        $value
    )

    if (-not (Test-Path $path))
    {
        if (-not $script:act)
        {
            $script:act = $true

            Write-Output ":: Updating Registry"
        }

        Write-Output "Create Registry Path: $path"

        New-Item -Path $path -Type Folder | Out-Null
    }

    if ((Get-ItemProperty -Path $path -Name $name | Select-Object -ExpandProperty $name) -ne [Environment]::ExpandEnvironmentVariables($value))
    {
        if (-not $script:act)
        {
            $script:act = $true

            Write-Output ":: Updating Registry"
        }

        Write-Output "Set Registry Value: $path $name $value"

        Set-ItemProperty -Path $path -Name $name -Value $value
    }
}