<#
.SYNOPSIS
    Test module entry point that re-exports functions from specialized test modules.

.DESCRIPTION
    This module maintains backward compatibility while organizing tests by type.
    It imports and re-exports test functions from specialized test modules.

    Modules:
        Test-StaticAnalysis.psm1  Static analysis tests (PSScriptAnalyzer)
#>

# Import and re-export static analysis tests
Import-Module "$PSScriptRoot/Test-StaticAnalysis.psm1" -Force
Export-ModuleMember -Function Test-PSScriptAnalyzer
