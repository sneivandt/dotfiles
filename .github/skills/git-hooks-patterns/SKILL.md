---
name: git-hooks-patterns
description: >
  Git hooks patterns and sensitive data detection for the dotfiles project.
  Use when working with git hooks, pre-commit checks, or security scanning.
metadata:
  author: sneivandt
  version: "1.0"
---

# Git Hooks Patterns

This skill provides guidance on git hooks implementation and sensitive data detection in the dotfiles project.

## Overview

The project uses git hooks to enforce security and quality standards before commits. Hooks are stored in the `hooks/` directory and automatically installed as symlinks in `.git/hooks/`.

## Available Hooks

### pre-commit Hook
**Purpose**: Scan staged changes for sensitive information before allowing commits.

**Location**: `hooks/pre-commit`

**Implementation**: POSIX shell script (`#!/bin/sh`)

**Detection Patterns**: Defined in `hooks/sensitive-patterns.ini`

## Hook Installation

### Automatic Installation
Hooks are automatically installed during dotfiles setup:

**Linux:**
```bash
./dotfiles.sh -I
```

**Windows:**
```powershell
.\dotfiles.ps1 -Install
```

### Implementation
The installation creates symlinks from `.git/hooks/` to `hooks/`:
```sh
ln -sf "../../hooks/pre-commit" ".git/hooks/pre-commit"
```

Benefits:
- Updates to hooks in the repository are immediately active
- No reinstallation needed when hooks change
- Version controlled hook logic

## Sensitive Data Detection

### Pattern Configuration Format

Patterns are defined in `hooks/sensitive-patterns.ini` using INI sections:

```ini
# Section headers organize patterns by type
[section-name]
pattern-one
pattern-two

[another-section]
more-patterns
```

### Pattern Categories

- **`api-keys`**: Generic API keys and secret keys
- **`passwords`**: Password assignments and configurations
- **`tokens`**: Bearer tokens, access tokens, authorization headers
- **`aws`**: AWS credentials (access keys, secret keys)
- **`private-keys`**: PEM-formatted private keys (RSA, DSA, EC, OpenSSH)
- **`github`**: GitHub personal access tokens
- **`gitlab`**: GitLab personal access tokens
- **`oauth`**: OAuth client secrets
- **`database`**: Database connection strings with credentials
- **`slack`**: Slack tokens and webhooks
- **`stripe`**: Stripe API keys
- **`google`**: Google Cloud and Firebase API keys
- **`heroku`**: Heroku API keys
- **`generic`**: High-entropy generic secrets

### Pattern Format

Patterns use Extended Regular Expressions (ERE):
- Case-insensitive matching
- No need for escape characters for most metacharacters
- Supports alternation: `(pattern1|pattern2)`
- Supports character classes: `[a-zA-Z0-9]`

### Example Patterns

```ini
[api-keys]
# Matches: apikey=xxx, api_key = xxx, API_KEY: xxx
(apikey|api_key)[\s]*[=:]

# Matches: secret_key = xxx
secret_key[\s]*[=:]

[passwords]
# Matches: password=xxx, passwd: xxx
(password|passwd|pwd)[\s]*[=:]

[tokens]
# Matches: Authorization: Bearer xxx
authorization[\s]*:[\s]*bearer[\s]+[a-z0-9._-]{20,}

[private-keys]
# Matches: -----BEGIN RSA PRIVATE KEY-----
-----BEGIN[\s]+(RSA|DSA|EC|OPENSSH)[\s]+PRIVATE[\s]+KEY-----
```

## Hook Behavior

### When Sensitive Data Detected

The hook prints an error message and aborts the commit:

```
ERROR: Potential sensitive information detected!
======================================================

In file: config/example.py
Pattern matched: (apikey|api_key)[\s]*[=:]

Commit aborted to prevent leaking sensitive data.
Please review and remove any sensitive information.
If this is a false positive, use:
  git commit --no-verify
```

### Bypassing the Hook

Use `--no-verify` flag to bypass (use with caution):
```bash
git commit --no-verify
```

**When to bypass**:
- False positives (e.g., example code, documentation)
- Testing or dummy credentials
- Pattern matches non-sensitive content

**Never bypass for**:
- Real API keys or tokens
- Actual passwords
- Production credentials
- Private keys

## Adding New Patterns

### 1. Identify the Pattern Type
Determine which section the pattern belongs to or create a new section.

### 2. Write the Pattern
Use Extended Regular Expression syntax:
```ini
[new-section]
# Match specific format
pattern[\s]*[=:][\s]*value

# Match alternatives
(option1|option2|option3)

# Match character ranges
[a-zA-Z0-9_-]+
```

### 3. Test the Pattern
Test with grep using ERE mode:
```bash
echo "apikey=secret123" | grep -E "(apikey|api_key)[\s]*[=:]"
```

### 4. Add to sensitive-patterns.ini
```ini
[section-name]
your-new-pattern
```

### 5. Test the Hook
Stage a test file and attempt to commit:
```bash
echo "apikey=test" > test.txt
git add test.txt
git commit -m "Test"  # Should be blocked
rm test.txt
```

## Pattern Guidelines

### Reduce False Positives
- Be specific: Match context around the pattern
- Use word boundaries when appropriate
- Consider common usage patterns
- Test against real files

### Increase Detection
- Consider variations: `api_key`, `apiKey`, `APIKEY`
- Match common delimiters: `=`, `:`, `" : "`
- Allow whitespace: `[\s]*`
- Match value patterns when possible

### Example: Good vs. Bad

**Bad** (too many false positives):
```ini
[passwords]
password
```

**Good** (specific context):
```ini
[passwords]
(password|passwd|pwd)[\s]*[=:]
```

## Cross-Platform Compatibility

### Shell Compatibility
Hooks use POSIX shell (`#!/bin/sh`):
- Compatible with Linux, macOS, Windows (Git Bash)
- No Bash-specific features
- Standard utilities only (grep, git)

### Testing on Windows
Git for Windows includes Git Bash:
```powershell
# Hook runs in Git Bash automatically
git commit -m "Test"
```

## Hook Development Patterns

### Script Structure
```sh
#!/bin/sh
set -o errexit  # Exit on error
set -o nounset  # Exit on undefined variable

# Configuration
PATTERNS_FILE="hooks/sensitive-patterns.ini"
REPO_ROOT="$(git rev-parse --show-toplevel)"

# Main logic
main() {
  # Get staged files
  files=$(git diff --cached --name-only --diff-filter=ACM)

  # Process files
  for file in $files; do
    # Check patterns
  done
}

main
```

### Reading INI Patterns
```sh
# Read all patterns from INI file (skip comments and blank lines)
patterns=$(grep -v '^#' "$PATTERNS_FILE" | grep -v '^\[' | grep -v '^$')

# Check each pattern
for pattern in $patterns; do
  if git diff --cached -- "$file" | grep -iE "$pattern" >/dev/null; then
    # Pattern matched
  fi
done
```

### Error Reporting
```sh
# Clear error message
cat <<EOF
ERROR: Potential sensitive information detected!
======================================================

In file: $file
Pattern matched: $pattern

Commit aborted to prevent leaking sensitive data.
EOF

exit 1  # Non-zero exit aborts commit
```

## Rules

- All hooks must use POSIX shell (`#!/bin/sh`)
- Hooks are installed as symlinks (changes apply immediately)
- Never commit real credentials (even in comments)
- Test hooks before committing changes
- Document patterns in sensitive-patterns.ini
- Provide clear error messages
- Allow bypass with `--no-verify` for false positives
- Maintain cross-platform compatibility
