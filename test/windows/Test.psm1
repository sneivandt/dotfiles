# -----------------------------------------------------------------------------
# Test.psm1
# -----------------------------------------------------------------------------
# Test module entry point that re-exports functions from specialized test modules.
# This maintains backward compatibility while organizing tests by type.
#
# Modules:
#   Test-StaticAnalysis.psm1  Static analysis tests (PSScriptAnalyzer)
# -----------------------------------------------------------------------------

# Import and re-export static analysis tests
Import-Module "$PSScriptRoot/Test-StaticAnalysis.psm1" -Force
Export-ModuleMember -Function Test-PSScriptAnalyzer
