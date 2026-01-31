# Contributing to Dotfiles

Thank you for your interest in contributing! This document provides guidelines and instructions for contributing to this project.

## Development Setup

### Prerequisites

- Git
- A Unix-like environment (Linux, macOS, WSL)
- ShellCheck for linting shell scripts
- Docker (optional, for testing)

### Initial Setup

1. **Fork and clone the repository**:
   ```bash
   git clone https://github.com/yourusername/dotfiles.git
   cd dotfiles
   ```

2. **Initialize submodules**:
   ```bash
   git submodule update --init --recursive
   ```

3. **Install ShellCheck** (if not already installed):
   ```bash
   # Ubuntu/Debian
   sudo apt-get install shellcheck
   
   # macOS
   brew install shellcheck
   
   # Arch Linux
   sudo pacman -S shellcheck
   ```

## Running Tests Locally

### Shell Script Validation

Run the built-in test suite:

```bash
./dotfiles.sh --test -v
```

This will:
- Initialize git submodules
- Run ShellCheck on all shell scripts
- Run PSScriptAnalyzer on PowerShell scripts (if pwsh is installed)

### Manual ShellCheck

To run ShellCheck on specific files:

```bash
shellcheck dotfiles.sh src/*.sh
```

### Test Installation (Non-Destructive)

Test the installation process in verbose mode:

```bash
./dotfiles.sh --install -v
```

**Note**: This will create symlinks in your home directory. Review changes carefully or test in a Docker container.

### Docker Testing

Build and test in an isolated Docker container:

```bash
docker build -t dotfiles-test .
docker run --rm -it dotfiles-test
```

## Shell Script Style Guidelines

### POSIX Compliance

- Use `#!/bin/sh` (not bash) unless bash-specific features are required
- Always start scripts with:
  ```bash
  #!/bin/sh
  set -o errexit
  set -o nounset
  ```

### Coding Standards

1. **Quoting**: Always quote variable expansions
   ```bash
   # Good
   if [ "$var" = "value" ]; then
   
   # Bad
   if [ $var = value ]; then
   ```

2. **Functions**: Document parameters and behavior
   ```bash
   # function_name
   #
   # Description of what the function does.
   #
   # Args:
   #   $1  description of first argument
   #
   # Result:
   #   0 success, 1 failure
   function_name()
   {
     # implementation
   }
   ```

3. **Subshells**: Task functions should run in subshells
   ```bash
   task_function()
   {(
     # implementation in subshell
   )}
   ```

4. **Idempotency**: Always check state before modifying
   ```bash
   if [ condition_already_met ]; then
     log_verbose "Skipping: already configured"
     return
   fi
   ```

5. **Logging**: Use provided helpers
   ```bash
   log_stage "Installing packages"
   log_verbose "Installing package: $pkg"
   log_error "Failed to install"
   ```

### ShellCheck Compliance

- Fix all ShellCheck warnings
- Use `# shellcheck disable=SC####` only when necessary with explanation
- Acceptable directives (with justification):
  - `SC2086`: Intentional word splitting
  - `SC2012`: Using ls output in specific contexts
  - `SC2034`: Variables used in sourced files

## Code Quality Requirements

All pull requests must:

1. ✅ Pass ShellCheck with no errors
2. ✅ Maintain idempotency (re-running should be safe)
3. ✅ Follow existing code style and conventions
4. ✅ Include appropriate log messages
5. ✅ Preserve backwards compatibility
6. ✅ Pass all CI checks

## Pull Request Process

### Before Submitting

1. **Test locally**:
   ```bash
   ./dotfiles.sh --test -v
   ```

2. **Check for ShellCheck errors**:
   ```bash
   shellcheck dotfiles.sh src/*.sh
   ```

3. **Verify changes are minimal**: Only modify files necessary for your feature/fix

4. **Update documentation**: If adding features, update README.md and relevant docs

### Submitting Your PR

1. **Create a feature branch**:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make your changes** with clear, atomic commits

3. **Write a descriptive PR title and description**:
   - Title: Brief summary (50 chars or less)
   - Description: 
     - What changed and why
     - How to test the changes
     - Any breaking changes or deprecations

4. **Ensure CI passes**: GitHub Actions will run tests automatically

### Review Process

- Maintainers will review your PR
- Address any feedback or requested changes
- Once approved, your PR will be merged

## How to Add a New Environment Layer

1. **Create the layer directory**:
   ```bash
   mkdir -p env/mylayer/symlinks
   ```

2. **Add configuration files** as needed:
   - `symlinks.conf`: List of symlinks (one per line)
   - `packages.conf`: System packages to install
   - `units.conf`: Systemd units to enable
   - `chmod.conf`: File permissions to set
   - `submodules.conf`: Git submodules to initialize

3. **Add files to symlink**:
   ```bash
   # Files in env/mylayer/symlinks/ will be symlinked to ~/.<path>
   echo "content" > env/mylayer/symlinks/myconfig
   echo "myconfig" >> env/mylayer/symlinks.conf
   ```

4. **Update layer logic** (if needed):
   - Edit `is_env_ignored()` in `src/utils.sh` if layer has dependencies
   - Example: Layer only for specific OS or requires flag

5. **Document the layer**:
   ```bash
   # Create env/mylayer/README.md
   echo "# My Layer" > env/mylayer/README.md
   echo "Purpose and contents of this layer" >> env/mylayer/README.md
   ```

6. **Test the layer**:
   ```bash
   ./dotfiles.sh --test -v
   ./dotfiles.sh --install -v  # Test installation
   ```

## Getting Help

- Open an issue for bugs or feature requests
- Check existing issues before creating new ones
- Be specific and provide examples when reporting issues

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (see LICENSE file).
