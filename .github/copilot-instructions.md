# GitHub Copilot Project Instructions

Core universal guidance for AI code assistants working on this dotfiles project.

## Project Overview

This project manages dotfiles and system configuration using a profile-based sparse checkout approach for Linux and Windows environments.

**Core Principles**:
- **Profile-Based**: Uses profiles (base, arch, arch-desktop, desktop, windows) to control sparse checkout and configuration
- **Idempotent**: All scripts can be safely re-run without side effects
- **Cross-Platform**: POSIX shell (`/bin/sh`) for Linux, PowerShell for Windows
- **Declarative**: Configuration in INI files, automatic installation

See `docs/ARCHITECTURE.md` for complete system design and repository structure.

## Working with This Codebase

### 1. Before Making Changes

**Always check relevant skills first** - this project uses GitHub Copilot Agent Skills for detailed technical patterns:
- `.github/skills/` - Agent-specific coding patterns and conventions
- See skill descriptions in the available skills list below

**For human context and procedures**, refer to documentation:
- `docs/CONTRIBUTING.md` - Contribution workflow
- `docs/ARCHITECTURE.md` - System design and structure
- `docs/CUSTOMIZATION.md` - Adding configuration items
- `docs/TROUBLESHOOTING.md` - Common issues and solutions

### 2. Code Quality

**File Formatting**: Never leave trailing whitespace at the end of lines. This applies to all file types.

**Testing**: Always run tests before committing:
```bash
./dotfiles.sh -T  # Runs shellcheck, PSScriptAnalyzer, and config validation
```

**Dry-Run**: Test changes safely:
```bash
./dotfiles.sh -I --dry-run  # Preview changes without applying
```

### 3. Security

**Before finalizing**:
- Run `code_review` tool for automated review
- Run `codeql_checker` tool to scan for vulnerabilities
- Never commit secrets, credentials, or sensitive data
- Fix any security issues found

## Documentation Structure

This project maintains clear separation between different documentation types:

- **`.github/copilot-instructions.md`** (this file) - Core universal agent guidance
- **`.github/skills/`** - Agent-specific technical patterns and coding conventions
- **`docs/`** - Human-readable guides and reference documentation (also useful for agents needing context)

When in doubt, check skills for technical patterns and docs for procedures and context.
