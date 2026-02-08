# Security Policy

## Supported Versions

This dotfiles project is actively maintained. Security updates are applied to the latest version in the `master` branch.

| Version | Supported          |
| ------- | ------------------ |
| master  | :white_check_mark: |

## Security Considerations

### Dotfiles and Sensitive Data

**Important**: This repository is designed for managing configuration files. Be aware of the following security considerations:

1. **Never commit sensitive data**:
   - API keys, tokens, passwords
   - SSH private keys
   - GPG private keys
   - Application secrets
   - Personal identification information

2. **Configuration files may contain**:
   - Usernames (redact if publishing publicly)
   - File paths revealing system structure
   - Software versions (potential vulnerability disclosure)

3. **Safe practices**:
   - Use environment variables for secrets
   - Use `.gitignore` to exclude sensitive files
   - Review commits before pushing
   - Use `git-secrets` or similar tools to scan for leaked credentials
   - Consider keeping sensitive configs in a private repository

### Script Execution Risks

The installation scripts in this repository execute with user privileges and perform system modifications:

1. **What the scripts do**:
   - Create symlinks in `$HOME` directory
   - Install system packages (requires sudo/admin)
   - Modify registry settings (Windows, requires elevation)
   - Enable systemd units (Linux)
   - Install fonts

2. **Before running**:
   - Review `dotfiles.sh` or `dotfiles.ps1` source code
   - Check `conf/*.ini` files for packages and settings
   - Use `--dry-run` mode to preview changes:
     ```bash
     ./dotfiles.sh -I --profile arch-desktop --dry-run
     ```
   - Run static analysis tests:
     ```bash
     ./dotfiles.sh -T
     ```

3. **Idempotency**:
   - Scripts are designed to be safe to re-run
   - Existing configurations are not backed up (by design)
   - Commit important files before running

### Windows-Specific Considerations

Windows scripts require elevation for:
- Registry modifications (HKCU console settings)
- Font installation
- Symlink creation (some directories)

**Risks**:
- Elevated scripts can modify system-wide settings
- Registry changes affect user experience
- Review `conf/registry.ini` before applying

### Linux-Specific Considerations

Linux scripts may use `sudo` for:
- Installing packages via pacman
- System-wide font installation

**Risks**:
- Package installation could introduce vulnerabilities
- Review `conf/packages.ini` before installing
- Ensure you trust package repositories

## Security Best Practices for Users

### Before Installation

1. **Fork and review**: Fork the repository and review code before running
2. **Test in a VM**: Test in a virtual machine or container first
3. **Backup**: Backup existing configuration files
4. **Dry run**: Always test with `--dry-run` first

### After Installation

1. **Audit**: Review created symlinks and installed packages
2. **Monitor**: Watch for unexpected system behavior
3. **Update**: Keep the repository updated
4. **Report**: Report any security issues found

### For Public Repositories

If you fork this repository publicly:

1. **Remove sensitive data**: Scrub all personal information
2. **Audit commits**: Review entire git history for leaked secrets
3. **Use git-filter-repo**: Remove sensitive data from history if needed
4. **Enable security features**: Enable GitHub security scanning
5. **Document changes**: Note any security-related modifications

## Secure Usage Examples

### Safe Secret Management

Instead of hardcoding secrets in config files:

```bash
# Bad - secret in file
export API_KEY="secret123"

# Good - read from secure location
export API_KEY="$(cat ~/.secrets/api_key)"

# Better - use system keyring
export API_KEY="$(secret-tool lookup service myapp key apikey)"
```

### Safe SSH Config

```
# Bad - overly permissive
Host *
    StrictHostKeyChecking no

# Good - explicit and secure
Host github.com
    User git
    IdentityFile ~/.ssh/id_ed25519
    StrictHostKeyChecking yes
```

### Safe Git Config

```ini
# Don't commit personal email if sharing publicly
[user]
    # Use environment variable or global config
    email = ${GIT_EMAIL}
```

## Additional Resources

- [OWASP Cheat Sheet Series](https://cheatsheetseries.owasp.org/)
- [GitHub Security Best Practices](https://docs.github.com/en/code-security)
- [git-secrets](https://github.com/awslabs/git-secrets) - Prevent committing secrets
- [truffleHog](https://github.com/trufflesecurity/trufflehog) - Find secrets in git history

## See Also

- [Git Hooks](HOOKS.md) - Pre-commit hook for detecting sensitive data
- [Contributing](CONTRIBUTING.md) - Security considerations for contributors
- [Usage Guide](USAGE.md) - Dry-run mode for safe testing
