# -----------------------------------------------------------------------------
# Test-ShellWrapper.ps1 - Tests for dotfiles.ps1 PowerShell wrapper
# -----------------------------------------------------------------------------

$ErrorActionPreference = 'Stop'

foreach ($testModule in @(
    'Helpers.ps1'
    'Test-Bootstrap.ps1'
    'Test-Attestation.ps1'
    'Test-ErrorAndIsolation.ps1'
    'Test-ArtifactsAndStartup.ps1'
)) {
    . (Join-Path $PSScriptRoot "windows/$testModule")
}

function Invoke-TestSuite {
    $results = @()

    $results += Test-BuildMode
    $results += Test-VersionDetection
    $results += Test-ChecksumVerification
    $results += Test-ArgumentForwarding
    $results += Test-UpdateArgumentForwarding
    $results += Test-AdvancedFlagForwarding
    $results += Test-VersionPinnedBootstrapUrl
    $results += Test-AttestationVerification
    $results += Test-CliStartupCompletionEnv
    $results += Test-UpdateRecovery
    $results += Test-ErrorHandling
    $results += Test-IsolatedWrapperPath
    $results += Test-NativeExitCode
    $results += Test-CargoArtifactPath
    $results += Test-CargoArtifactFailure
    $results += Test-ReleaseRepository

    $passed = ($results | Where-Object { $_ -eq $true }).Count
    $total = $results.Count

    Write-Output ""
    Write-Output "======================================="
    Write-Output "Results: $passed/$total tests passed"

    if ($passed -eq $total) {
        exit 0
    } else {
        exit 1
    }
}

# Run tests if executed directly
if ($MyInvocation.InvocationName -ne '.') {
    Invoke-TestSuite
}
