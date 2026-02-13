<#
.SYNOPSIS
    GitHub Copilot CLI skill management for Windows dotfiles
.DESCRIPTION
    Downloads and installs GitHub Copilot CLI skill folders from GitHub URLs
    listed in configuration. Skills are downloaded to $HOME/.copilot/skills/
    directory. Supports profile-based filtering.
.NOTES
    Admin: Not required
#>

function Install-CopilotSkills
{
    <#
    .SYNOPSIS
        Install GitHub Copilot CLI Skills
    .DESCRIPTION
        Reads GitHub Copilot CLI skill folder URLs from conf/copilot-skills.ini and downloads
        entire skill folders to ~/.copilot/skills/ directory. Supports profile-based
        sections for filtering skills by category (e.g., [base], [windows]).
    .PARAMETER root
        Repository root directory
    .PARAMETER excludedCategories
        Comma-separated list of categories to exclude (from profile)
    .PARAMETER DryRun
        When specified, logs actions that would be taken without making modifications
    #>
    # Plural name justified: function installs multiple skills as batch operation
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute("PSUseSingularNouns", "")]
    [CmdletBinding()]
    param (
        [Parameter(Mandatory = $true)]
        [string]
        $root,

        [Parameter(Mandatory = $false)]
        [string]
        $excludedCategories = "",

        [Parameter(Mandatory = $false)]
        [switch]
        $DryRun
    )

    $configFile = Join-Path $root "conf\copilot-skills.ini"
    Write-VerboseMessage "Reading Copilot CLI skill configuration from: conf/copilot-skills.ini"

    if (-not (Test-Path $configFile))
    {
        Write-VerboseMessage "Skipping Copilot CLI skills: no copilot-skills.ini found"
        return
    }

    # Check if any sections match the active profile
    if (-not (Test-HasMatchingSections -FilePath $configFile -ExcludedCategories $excludedCategories))
    {
        Write-VerboseMessage "Skipping Copilot CLI skills: no sections match current profile"
        return
    }

    Write-ProgressMessage -Message "Checking Copilot CLI skills..."

    # Get all sections from the config file
    $content = Get-Content $configFile
    $sections = @()
    foreach ($line in $content)
    {
        if ($line -match '^\[(.+)\]$')
        {
            $sections += $matches[1]
        }
    }

    Write-VerboseMessage "Found $($sections.Count) section(s) in copilot-skills.ini: $($sections -join ', ')"

    # Collect all skill URLs that should be downloaded based on profile
    $skillsToInstall = @()
    foreach ($section in $sections)
    {
        Write-VerboseMessage "Processing skills section: [$section]"

        # Check if this section should be included based on profile
        if (-not (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excludedCategories))
        {
            Write-VerboseMessage "Skipping Copilot CLI skills section [$section]: profile not included"
            continue
        }

        # Read skill URLs from this section
        $sectionSkills = Read-IniSection -FilePath $configFile -SectionName $section
        Write-VerboseMessage "Found $($sectionSkills.Count) skill(s) in section [$section]"
        $skillsToInstall += $sectionSkills
    }

    if ($skillsToInstall.Count -eq 0)
    {
        Write-VerboseMessage "Skipping Copilot CLI skills: no skills configured for current profile"
        return
    }

    # Remove duplicates if same skill appears in multiple sections
    $originalCount = $skillsToInstall.Count
    $skillsToInstall = $skillsToInstall | Select-Object -Unique
    if ($skillsToInstall.Count -lt $originalCount)
    {
        Write-VerboseMessage "Removed $($originalCount - $skillsToInstall.Count) duplicate skill(s)"
    }
    Write-VerboseMessage "Total skills to process: $($skillsToInstall.Count)"

    # Ensure skills directory exists
    $skillsDir = Join-Path $HOME ".copilot\skills"

    $act = $false

    # Helper function to recursively download folder contents
    function Download-GitHubFolder
    {
        param (
            [string]$owner,
            [string]$repo,
            [string]$branch,
            [string]$apiPath,
            [string]$targetPath,
            [ref]$filesDownloaded,
            [ref]$actRef
        )

        $apiUrl = "https://api.github.com/repos/$owner/$repo/contents/$apiPath`?ref=$branch"
        Write-VerboseMessage "Fetching contents from: $apiPath"

        try
        {
            $response = Invoke-RestMethod -Uri $apiUrl -ErrorAction Stop

            foreach ($item in $response)
            {
                if ($item.type -eq "file")
                {
                    $fileName = $item.name
                    $downloadUrl = $item.download_url
                    $filePath = Join-Path $targetPath $fileName

                    # Ensure directory exists
                    $fileDir = Split-Path $filePath -Parent
                    if (-not (Test-Path $fileDir))
                    {
                        New-Item -ItemType Directory -Path $fileDir -Force | Out-Null
                    }

                    Write-VerboseMessage "Downloading file: $fileName"

                    # Download to temporary file first
                    $tempFile = [System.IO.Path]::GetTempFileName()
                    Invoke-WebRequest -Uri $downloadUrl -OutFile $tempFile -ErrorAction Stop

                    # Check if file exists and content is different
                    $shouldUpdate = $true
                    if (Test-Path $filePath)
                    {
                        $existingContent = Get-Content -Path $filePath -Raw -ErrorAction SilentlyContinue
                        $newContent = Get-Content -Path $tempFile -Raw -ErrorAction SilentlyContinue
                        if ($existingContent -eq $newContent)
                        {
                            Write-VerboseMessage "Skipping file $fileName`: no changes"
                            $shouldUpdate = $false
                        }
                    }

                    if ($shouldUpdate)
                    {
                        if (-not $actRef.Value)
                        {
                            $actRef.Value = $true
                            Write-Stage -Message "Installing Copilot CLI Skills"
                        }
                        Move-Item -Path $tempFile -Destination $filePath -Force
                        $relativePath = $filePath.Substring($targetPath.Length).TrimStart('\')
                        Write-VerboseMessage "Installed file: $relativePath"
                        $filesDownloaded.Value++
                    }
                    else
                    {
                        Remove-Item -Path $tempFile -Force -ErrorAction SilentlyContinue
                    }
                }
                elseif ($item.type -eq "dir")
                {
                    $subPath = $item.path
                    $subTargetPath = Join-Path $targetPath $item.name
                    Write-VerboseMessage "Processing subdirectory: $($item.name)"

                    # Recursively download subdirectory
                    Download-GitHubFolder -owner $owner -repo $repo -branch $branch -apiPath $subPath -targetPath $subTargetPath -filesDownloaded $filesDownloaded -actRef $actRef
                }
            }
        }
        catch
        {
            Write-Warning "Failed to fetch contents from $apiUrl`: $_"
        }
    }

    foreach ($url in $skillsToInstall)
    {
        # Parse GitHub URL to extract components
        # Example: https://github.com/user/repo/blob/main/path/folder
        #      or: https://github.com/user/repo/tree/main/path/folder
        if ($url -notmatch 'github\.com/([^/]+)/([^/]+)/(blob|tree)/([^/]+)/(.+)')
        {
            Write-Warning "Invalid GitHub URL format: $url"
            continue
        }

        $owner = $matches[1]
        $repo = $matches[2]
        $branch = $matches[4]
        $folderPath = $matches[5]

        # Extract folder name from path (last segment)
        $folderName = Split-Path $folderPath -Leaf
        $targetDir = Join-Path $skillsDir $folderName

        if ($DryRun)
        {
            if (-not $act)
            {
                $act = $true
                Write-Stage -Message "Installing Copilot CLI Skills"
            }
            Write-DryRunMessage -Message "Would create directory: $targetDir"
            Write-DryRunMessage -Message "Would download skill folder from $url (including subdirectories)"
            Add-Counter -CounterName "copilot_skills_installed"
        }
        else
        {
            Write-VerboseMessage "Downloading skill folder from $url"

            try
            {
                # Ensure target directory exists
                if (-not (Test-Path $targetDir))
                {
                    New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
                }

                $filesDownloaded = 0
                $actRef = [ref]$act

                # Recursively download folder contents
                Download-GitHubFolder -owner $owner -repo $repo -branch $branch -apiPath $folderPath -targetPath $targetDir -filesDownloaded ([ref]$filesDownloaded) -actRef $actRef

                $act = $actRef.Value

                if ($filesDownloaded -gt 0)
                {
                    Write-VerboseMessage "Installed skill: $folderName ($filesDownloaded file(s))"
                    Add-Counter -CounterName "copilot_skills_installed"
                }
                else
                {
                    Write-VerboseMessage "Skipping skill $folderName`: no changes"
                }
            }
            catch
            {
                Write-Warning "Failed to download skill folder from $url`: $_"
            }
        }
    }
}
Export-ModuleMember -Function Install-CopilotSkills
