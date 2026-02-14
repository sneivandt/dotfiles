---
name: powershell-patterns
description: >
  PowerShell scripting patterns and conventions for the dotfiles project.
  Use when creating or modifying PowerShell scripts in src/windows/.
metadata:
  author: sneivandt
  version: "1.0"
---

# PowerShell Patterns

This skill provides PowerShell scripting patterns and conventions used in the dotfiles project.

## Code Style

- Match existing style: Verb-Noun function names
- Use comment-based help
- Export only necessary functions via `Export-ModuleMember`
- Windows automation should fail gracefully when run without elevation if elevation is required

## Logging Conventions

### Stage Headers
```powershell
Write-Output ":: Stage Name"
```
- Use `::` prefix
- Print only once per stage using `$act` flag
- Set `$act = $true` after first action

### Action Types
- Dry-run actions: `Write-Output "DRY-RUN: Would <action>"` (shown when `-DryRun` is set)
- Verbose details: `Write-Verbose "<message>"` for routine operations
- Skipping actions: `Write-Verbose "Skipping <item>: <reason>"`

**Note**: Logging output does not automatically prevent actions. Always check the `$DryRun` switch before performing system modifications. The `Write-Output "DRY-RUN: ..."` pattern is a convention for clarity, but you must still wrap actual work in the `else` block:

### Stage Logging Pattern
```powershell
$act = $false

foreach ($item in $items) {
  if ($shouldProcess) {
    if (-not $act) {
      $act = $true
      Write-Output ":: Stage Name"
    }
    # Process item
  }
}
```

## Idempotency

Check if action is needed before taking it:
- Check file existence, registry values, installed packages/extensions
- Skip with verbose message if already correct:
  ```powershell
  Write-Verbose "Skipping <item>: already <state>"
  ```

## Dry-Run Pattern

All functions support `-DryRun` switch:
```powershell
function Install-Something {
  [CmdletBinding()]
  param (
    [Parameter(Mandatory = $true)]
    [string]$Item,

    [Parameter(Mandatory = $false)]
    [switch]$DryRun
  )

  if ($DryRun) {
    Write-Output "DRY-RUN: Would install $Item"
  } else {
    Write-Verbose "Installing $Item"
    # actual work
  }
}
```

- Check `if ($DryRun)` before any system modification
- Log intended action with `Write-Output "DRY-RUN: Would <action>"`
- Never modify system state when `$DryRun` is set

## INI Parsing

Always use `Read-IniSection` helper from `Profile.psm1` instead of manual parsing:
```powershell
$fonts = Read-IniSection -FilePath $configFile -SectionName "fonts"
```
- Reads a specific section from an INI file
- Returns array of non-empty, non-comment lines

## Profile Filtering

Use `Test-ShouldIncludeSection` to check if a section should be processed:
```powershell
if (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excludedCategories) {
  # Process this section
}
```
- Returns `$true` if ALL required categories in section name are NOT excluded

Use `Get-ProfileExclusion` to resolve profile to excluded categories in main script:
```powershell
$excludedCategories = Get-ProfileExclusion -Profile $profile
```

See the `profile-system` skill for details on how profile filtering works and section naming conventions.

## Configuration Processing Pattern

```powershell
# Reading from config
$items = Read-IniSection -FilePath $configFile -SectionName "section"

# Get all sections
$content = Get-Content $configFile
$sections = @()
foreach ($line in $content) {
  if ($line -match '^\[(.+)\]$') {
    $sections += $matches[1]
  }
}

# Process each section
foreach ($section in $sections) {
  # Checking sections
  if (-not (Test-ShouldIncludeSection -SectionName $section -ExcludedCategories $excludedCategories)) {
    Write-Verbose "Skipping section [$section]: profile not included"
    continue
  }

  # Process items in section
  $items = Read-IniSection -FilePath $configFile -SectionName $section
  foreach ($item in $items) {
    # Process item
  }
}
```

## Error Suppression

Use `-ErrorAction SilentlyContinue` only when appropriate, prefer explicit checks:
```powershell
# Prefer explicit check
if (Test-Path $path) {
  $content = Get-Content $path
}

# Instead of
$content = Get-Content $path -ErrorAction SilentlyContinue
```

## Configuration Format

Configuration files in `conf/` follow these patterns:
- **`symlinks.ini`**: Uses `[windows]` section with paths relative to `symlinks/` (no leading dot)
  - Well-known Windows folders (AppData, Documents, etc.) remain as-is in target
  - Unix-style paths (config, ssh) get prefixed with dot by Symlinks.psm1
- **`registry.ini`**: Registry paths as sections with `name = value` format
  - Section headers are registry paths (e.g., `[HKCU:\Software\Example]`)
  - No profile filtering (Windows-only by nature, all settings applied on Windows)
- **All other INI files**: Follow section-based format like Linux (e.g., `[windows]`, `[base]`)

## Rules

- Use Verb-Noun function names with approved PowerShell verbs
- Include comment-based help for exported functions
- Export only necessary functions via `Export-ModuleMember`
- Support `-DryRun` switch for all functions that modify system state
- Use `Write-Output` for stage headers with `::` prefix
- Use `Write-Verbose` for routine operations and skipped items
- Check idempotency before performing actions
- Always use `Read-IniSection` helper for INI parsing
- Use `Test-ShouldIncludeSection` for profile filtering
- Prefer explicit checks over `-ErrorAction SilentlyContinue`
