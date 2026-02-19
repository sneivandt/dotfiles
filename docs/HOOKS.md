# Repository Git Hooks

The `hooks/` directory contains git hooks that are automatically installed by the dotfiles installation script.

## Available Hooks

### pre-commit - Orchestrator

A thin delegator that runs each check script in sequence. Adding a new check
is as simple as dropping a new `check-*.sh` script in `hooks/` and calling it
from `pre-commit`.

### check-sensitive.sh - Sensitive Data Scanner

Scans staged changes for sensitive information before allowing commits. Detects:

- **API Keys**: apikey, api_key, secret_key patterns
- **Passwords**: password, passwd, pwd assignments
- **Tokens**: OAuth tokens, access tokens, JWT tokens
- **Private Keys**: PEM-formatted private keys (RSA, DSA, EC, OpenSSH)
- **AWS Credentials**: AWS access keys and secret keys
- **GitHub/GitLab Tokens**: Personal access tokens
- **Database Credentials**: Connection strings with embedded credentials
- **Cloud Provider Keys**: Google Cloud, Stripe, Slack, Heroku
- **Generic Secrets**: High-entropy strings in secret-related variables

#### Usage

The hook runs automatically on every commit in this repository. If sensitive data is detected:

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

To bypass the hook (use with caution):
```bash
git commit --no-verify
```

### check-rust.sh - Rust Code Quality

Runs two checks in order when any `.rs` files are staged:

1. **`cargo fmt --check`** — fails the commit if any files are not formatted.
   Run `cargo fmt --manifest-path cli/Cargo.toml` to fix.
2. **`cargo clippy -- -D warnings`** — fails the commit if clippy reports any
   warnings, matching the same lint policy enforced by CI.

#### Customization

The detection patterns are defined in [sensitive-patterns.ini](../hooks/sensitive-patterns.ini), organized into sections by pattern type:
- `api-keys` - Generic API keys and secrets
- `passwords` - Password patterns
- `tokens` - Bearer tokens and authorization headers
- `aws` - AWS credentials
- `private-keys` - PEM-formatted private keys
- `github` - GitHub personal access tokens
- `gitlab` - GitLab personal access tokens
- `oauth` - OAuth client secrets
- `database` - Database connection strings with credentials
- `slack` - Slack tokens
- `stripe` - Stripe API keys
- `google` - Google Cloud and Firebase API keys
- `heroku` - Heroku API keys
- `generic` - High-entropy generic secrets

The INI file uses a simple, clean format with raw regex patterns under section headers. The file includes comprehensive documentation about:
- Pattern format (Extended Regular Expressions)
- How to add new patterns
- Testing patterns
- Pattern guidelines to reduce false positives

Edit `hooks/sensitive-patterns.ini` to add, modify, or remove detection patterns. The section-based organization makes it easy to understand and manage different types of secrets. Changes take effect immediately since the hook file is symlinked.

## File Layout

| File | Installed as git hook | Purpose |
|---|---|---|
| `pre-commit` | yes | Orchestrator — calls each check script |
| `check-sensitive.sh` | no | Sensitive data scanning |
| `check-rust.sh` | no | Rust formatting and clippy linting |
| `sensitive-patterns.ini` | no | Regex patterns for sensitive data scanner |

Only files without an extension are installed as git hooks. Helper scripts use
the `.sh` extension so they live alongside the hook without being linked
into `.git/hooks/`.

## Installation

Hooks are automatically installed when you run the dotfiles installation:

**Linux:**
```bash
./dotfiles.sh install
```

**Windows:**
```powershell
.\dotfiles.ps1 install -p desktop
```

The binary creates a symlink from `.git/hooks/pre-commit` to `hooks/pre-commit`, so any updates to the hook in the repository are automatically reflected without reinstalling.

## Cross-Platform Compatibility

Hooks are written in POSIX shell (`#!/bin/sh`) and work on:
- **Linux**: Native shell support
- **Windows**: Git for Windows includes Git Bash
- **macOS**: Native shell support

## See Also

- [Architecture](ARCHITECTURE.md) - Git hooks installation process
- [Security](SECURITY.md) - Security best practices and sensitive data handling
- [Contributing](CONTRIBUTING.md) - Guidelines for hook development
