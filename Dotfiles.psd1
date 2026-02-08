@{
    # Script module or binary module file associated with this manifest
    RootModule = 'Dotfiles.psm1'

    # Version number of this module
    ModuleVersion = '1.0.0'

    # ID used to uniquely identify this module
    GUID = 'a1b2c3d4-e5f6-7890-1234-567890abcdef'

    # Author of this module
    Author = 'sneivandt'

    # Company or vendor of this module
    CompanyName = 'Unknown'

    # Copyright statement for this module
    Copyright = '(c) sneivandt. All rights reserved.'

    # Description of the functionality provided by this module
    Description = 'Dotfiles management module for Windows. Provides commands to install and update dotfiles configuration.'

    # Minimum version of the PowerShell engine required by this module
    PowerShellVersion = '5.1'

    # Functions to export from this module
    FunctionsToExport = @('Install-Dotfiles', 'Update-Dotfiles')

    # Cmdlets to export from this module
    CmdletsToExport = @()

    # Variables to export from this module
    VariablesToExport = @()

    # Aliases to export from this module
    AliasesToExport = @()

    # Private data to pass to the module specified in RootModule/ModuleToProcess
    PrivateData = @{
        PSData = @{
            # Tags applied to this module
            Tags = @('dotfiles', 'configuration', 'windows')

            # A URL to the license for this module
            LicenseUri = 'https://github.com/sneivandt/dotfiles/blob/master/LICENSE'

            # A URL to the main website for this project
            ProjectUri = 'https://github.com/sneivandt/dotfiles'
        }
    }
}
