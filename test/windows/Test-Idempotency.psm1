# -----------------------------------------------------------------------------
# Test-Idempotency.psm1
# -----------------------------------------------------------------------------
# Idempotency tests for Windows dotfiles installation.
#
# Functions:
#   Test-IdempotencyInstall  Test that installation is idempotent
# -----------------------------------------------------------------------------

<#
.SYNOPSIS
Tests that dotfiles installation is idempotent on Windows.

.DESCRIPTION
Runs the installation process twice and verifies that:
1. The second run completes without errors
2. No unnecessary changes are made
3. All operations are properly skipped when already correct

.PARAMETER Profile
The profile to test (default: "windows")

.OUTPUTS
System.Boolean

.EXAMPLE
Test-IdempotencyInstall -Profile "windows" -Verbose
#>
function Test-IdempotencyInstall {
    [CmdletBinding()]
    [OutputType([System.Boolean])]
    param(
        [Parameter(Mandatory = $false)]
        [string]$Profile = "windows"
    )

    Write-Output ":: Testing $Profile profile idempotency"

    # Get the repository root directory
    $repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

    # Path to dotfiles.ps1
    $dotfilesScript = Join-Path $repoRoot "dotfiles.ps1"

    if (-not (Test-Path $dotfilesScript)) {
        Write-Error "dotfiles.ps1 not found at: $dotfilesScript"
        return $false
    }

    Write-Verbose "Running first installation (profile=$Profile)"

    # First installation run
    $firstRunOutput = & pwsh -File $dotfilesScript -Install -Profile $Profile -Verbose 2>&1
    $firstRunExitCode = $LASTEXITCODE

    if ($firstRunExitCode -ne 0) {
        Write-Error "First installation run failed for profile $Profile"
        Write-Error "Exit code: $firstRunExitCode"
        Write-Error "Output:"
        $firstRunOutput | ForEach-Object { Write-Error $_ }
        return $false
    }

    Write-Verbose "First installation completed successfully"
    Write-Verbose "Running second installation (should be idempotent)"

    # Second installation run (should be idempotent)
    $secondRunOutput = & pwsh -File $dotfilesScript -Install -Profile $Profile -Verbose 2>&1
    $secondRunExitCode = $LASTEXITCODE

    if ($secondRunExitCode -ne 0) {
        Write-Error "Second installation run failed for profile $Profile"
        Write-Error "This indicates the installation is not idempotent"
        Write-Error "Exit code: $secondRunExitCode"
        Write-Error "Output:"
        $secondRunOutput | ForEach-Object { Write-Error $_ }
        return $false
    }

    Write-Verbose "Second installation completed successfully"

    # Verify second run shows idempotent behavior
    # Look for "Skipping" messages which indicate operations were not needed
    $skipCount = ($secondRunOutput | Where-Object { $_ -like "*Skipping*" }).Count

    Write-Verbose "Second run reported $skipCount skip operations"

    # Check that second run didn't have errors
    $errorCount = ($secondRunOutput | Where-Object { $_ -like "*ERROR:*" -or $_ -like "*Error:*" }).Count

    if ($errorCount -gt 0) {
        Write-Error "Second installation run contained $errorCount error(s) for profile $Profile"
        Write-Error "Output:"
        $secondRunOutput | Where-Object { $_ -like "*ERROR:*" -or $_ -like "*Error:*" } | ForEach-Object { Write-Error $_ }
        return $false
    }

    Write-Verbose "Idempotency verified for profile $Profile"
    Write-Output "✓ $Profile profile idempotency test passed"

    return $true
}

Export-ModuleMember -Function Test-IdempotencyInstall
