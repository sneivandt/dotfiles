#Requires -PSEdition Desktop

function Install-VsCodeExtensions
{
    <#
    .SYNOPSIS
        Install VS Code Extensions
    .DESCRIPTION
        Reads VS Code extensions from conf/vscode-extensions.ini [extensions]
        section and ensures they are installed. Checks both code and
        code-insiders binaries if available.
    .PARAMETER root
        Repository root directory
    .PARAMETER DryRun
        When specified, logs actions that would be taken without making modifications
    #>
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

    $configFile = Join-Path $root "conf\vscode-extensions.ini"

    if (-not (Test-Path $configFile))
    {
        Write-Verbose "Skipping VS Code extensions: no vscode-extensions.ini found"
        return
    }

    # Read extensions from [extensions] section using helper
    $extensions = Read-IniSection -FilePath $configFile -SectionName "extensions"

    if ($extensions.Count -eq 0)
    {
        Write-Verbose "Skipping VS Code extensions: no extensions configured"
        return
    }

    # Iterate over both stable and insiders versions of VS Code
    foreach ($code in @('code', 'code-insiders'))
    {
        # Check if the code binary exists
        if (-not (Get-Command $code -ErrorAction SilentlyContinue))
        {
            Write-Verbose "Skipping $code`: not installed"
            continue
        }

        $act = $false

        # Get list of currently installed extensions to avoid redundant calls
        $installed = & $code --list-extensions

        foreach ($extension in $extensions)
        {
            # Check if extension is already installed
            if ($installed -notcontains $extension)
            {
                if (-not $act)
                {
                    $act = $true
                    Write-Output ":: Installing $code Extensions"
                }

                if ($DryRun)
                {
                    Write-Output "DRY-RUN: Would install extension: $extension"
                }
                else
                {
                    Write-Verbose "Installing extension: $extension"
                    & $code --install-extension $extension
                }
            }
            else
            {
                Write-Verbose "Skipping $code extension $extension`: already installed"
            }
        }
    }
}
Export-ModuleMember -Function Install-VsCodeExtensions