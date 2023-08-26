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
    )

    $script:act = $false

    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "NormalForeground"    -Value 0xF
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "CommentForeground"   -Value 0x7
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "KeywordForeground"   -Value 0x1
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "StringForeground"    -Value 0xA
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "OperatorForeground"  -Value 0xB
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "VariableForeground"  -Value 0xB
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "CommandForeground"   -Value 0x1
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "ParameterForeground" -Value 0xF
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "TypeForeground"      -Value 0xE
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "NumberForeground"    -Value 0xC
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "MemberForeground"    -Value 0xE
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "EmphasisForeground"  -Value 0xD
    Set-RegistryValue -Path "HKCU:\Console\PSReadLine" -Name "ErrorForeground"     -Value 0x4

    @(`
        "HKCU:\Console\%SystemRoot%_System32_bash.exe", `
        "HKCU:\Console\%SystemRoot%_System32_WindowsPowerShell_v1.0_powershell.exe", `
        "HKCU:\Console\%SystemRoot%_SysWOW64_WindowsPowerShell_v1.0_powershell.exe", `
        "HKCU:\Console\Windows PowerShell (x86)", `
        "HKCU:\Console\Windows PowerShell", `
        "HKCU:\Console"`
    ) | ForEach-Object {

        Set-RegistryValue -Path $_ -Name "WindowSize"        -Value 0x00200078 # 32h x 120w
        Set-RegistryValue -Path $_ -Name "ScreenBufferSize"  -Value 0x0BB80078 # 3000h x 120w
        Set-RegistryValue -Path $_ -Name "CursorSize"        -Value 100
        Set-RegistryValue -Path $_ -Name "FaceName"          -Value "DejaVu Sans Mono for Powerline"
        Set-RegistryValue -Path $_ -Name "FontFamily"        -Value 54
        Set-RegistryValue -Path $_ -Name "FontSize"          -Value 0x00100000 # 16px height x auto width
        Set-RegistryValue -Path $_ -Name "FontWeight"        -Value 400
        Set-RegistryValue -Path $_ -Name "HistoryBufferSize" -Value 50
        Set-RegistryValue -Path $_ -Name "HistoryNoDup"      -Value 1
        Set-RegistryValue -Path $_ -Name "InsertMode"        -Value 1
        Set-RegistryValue -Path $_ -Name "QuickEdit"         -Value 1
        Set-RegistryValue -Path $_ -Name "ScreenColors"      -Value 0x0F
        Set-RegistryValue -Path $_ -Name "PopupColors"       -Value 0xF0
        Set-RegistryValue -Path $_ -Name "WindowAlpha"       -Value 0xFF

        Set-RegistryValue -Path $_ -Name "ColorTable00"      -Value (Convert-ConsoleColor "#151515") # Black
        Set-RegistryValue -Path $_ -Name "ColorTable01"      -Value (Convert-ConsoleColor "#8197bf") # DarkBlue
        Set-RegistryValue -Path $_ -Name "ColorTable02"      -Value (Convert-ConsoleColor "#437019") # DarkGreen
        Set-RegistryValue -Path $_ -Name "ColorTable03"      -Value (Convert-ConsoleColor "#556779") # DarkCyan
        Set-RegistryValue -Path $_ -Name "ColorTable04"      -Value (Convert-ConsoleColor "#902020") # DarkRed
        Set-RegistryValue -Path $_ -Name "ColorTable05"      -Value (Convert-ConsoleColor "#540063") # DarkMagenta
        Set-RegistryValue -Path $_ -Name "ColorTable06"      -Value (Convert-ConsoleColor "#dad085") # DarkYellow
        Set-RegistryValue -Path $_ -Name "ColorTable07"      -Value (Convert-ConsoleColor "#888888") # Gray
        Set-RegistryValue -Path $_ -Name "ColorTable08"      -Value (Convert-ConsoleColor "#606060") # DarkGray
        Set-RegistryValue -Path $_ -Name "ColorTable09"      -Value (Convert-ConsoleColor "#7697d6") # Blue
        Set-RegistryValue -Path $_ -Name "ColorTable10"      -Value (Convert-ConsoleColor "#99ad6a") # Green
        Set-RegistryValue -Path $_ -Name "ColorTable11"      -Value (Convert-ConsoleColor "#c6b6ee") # Cyan
        Set-RegistryValue -Path $_ -Name "ColorTable12"      -Value (Convert-ConsoleColor "#cf6a4c") # Red
        Set-RegistryValue -Path $_ -Name "ColorTable13"      -Value (Convert-ConsoleColor "#f0a0c0") # Magenta
        Set-RegistryValue -Path $_ -Name "ColorTable14"      -Value (Convert-ConsoleColor "#fad07a") # Yellow
        Set-RegistryValue -Path $_ -Name "ColorTable15"      -Value (Convert-ConsoleColor "#e8e8d3") # White
    }

    Set-RegistryValue -Path "HKCU:\Control Panel\International"                                            -Name "sLongDate"                           -Value "MMMM d, yyyy"
    Set-RegistryValue -Path "HKCU:\Control Panel\International"                                            -Name "sShortDate"                          -Value "MM/dd/yy"
    Set-RegistryValue -Path "HKCU:\Control Panel\International"                                            -Name "sShortTime"                          -Value "HH:mm"
    Set-RegistryValue -Path "HKCU:\Control Panel\International"                                            -Name "sTimeFormat"                         -Value "HH:mm:ss"
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\TabletTip\1.7"                                       -Name "TipbandDesiredVisibility"            -Value 0
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer"                     -Name "EnableAutoTray"                      -Value 0
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"            -Name "Hidden"                              -Value 1
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"            -Name "HideFileExt"                         -Value 0
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"            -Name "LaunchTo"                            -Value 1
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"            -Name "ShowTaskViewButton"                  -Value 0
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced\People"     -Name "PeopleBand"                          -Value 0
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\CabinetState"        -Name "FullPath"                            -Value 1
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders"  -Name "Personal"                            -Value "%USERPROFILE%\Documents"
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\PenWorkspace"                 -Name "PenWorkspaceButtonDesiredVisibility" -Value 0
    Set-RegistryValue -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Search"                       -Name "SearchboxTaskbarMode"                -Value 0
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