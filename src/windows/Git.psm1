<#
.SYNOPSIS
    Git configuration utilities for Windows dotfiles
.DESCRIPTION
    Configures Git settings to ensure smooth operation on Windows, particularly
    around symlink handling which can cause permission issues if not properly
    configured.
.NOTES
    Admin: Not required
#>

function Initialize-GitConfig
{
    <#
    .SYNOPSIS
        Configure Git settings for Windows compatibility
    .DESCRIPTION
        Sets core.symlinks=false to treat repository symlinks as text files
        containing the target path. This prevents permission errors during
        git pull and checkout operations on Windows.

        Idempotent - only sets the value if not already configured.
    .PARAMETER Root
        Root directory of the dotfiles repository
    .PARAMETER DryRun
        If specified, shows what would be done without making changes
    .EXAMPLE
        Initialize-GitConfig -Root $PSScriptRoot -DryRun
        Shows what Git configuration would be applied
    #>
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $Root,

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    # Track if we've printed the stage header
    $act = $false

    # Check current symlinks setting
    Push-Location $Root
    try
    {
        $currentSymlinks = git config --local --get core.symlinks 2>$null

        if ($currentSymlinks -ne 'false')
        {
            if (-not $act)
            {
                $act = $true
                Write-Output ":: Git Configuration"
            }

            if ($DryRun)
            {
                Write-Output "DRY-RUN: Would set git config core.symlinks false"
            }
            else
            {
                Write-Verbose "Setting core.symlinks = false"
                git config --local core.symlinks false
            }
        }
        else
        {
            Write-Verbose "Git core.symlinks already configured"
        }
    }
    finally
    {
        Pop-Location
    }
}

Export-ModuleMember -Function Initialize-GitConfig
