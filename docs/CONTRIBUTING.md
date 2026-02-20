# Contributing to Dotfiles

Thank you for your interest in contributing! This document provides guidelines for contributing to this dotfiles project.

## Getting Started

1. Fork the repository
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/dotfiles.git
   cd dotfiles
   ```
3. Create a feature branch:
   ```bash
   git checkout -b feature/your-feature-name
   ```

**Before making changes**, familiarize yourself with:
- [Architecture Documentation](ARCHITECTURE.md) - Understanding the system design
- [Profile System](PROFILES.md) - How profiles work
- [Configuration Reference](CONFIGURATION.md) - Configuration file formats
- [Testing Documentation](TESTING.md) - How to test your changes

**Agent Skills**: For technical coding patterns, GitHub Copilot uses skills in `.github/skills/`. See `.github/copilot-instructions.md` for the complete list.

## Prerequisites

Install the Rust toolchain via [rustup](https://rustup.rs/):

- **Linux / macOS**: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Windows**: `winget install Rustlang.Rustup`

After installation, ensure `cargo` is on your PATH (open a new terminal if needed). The project targets the **stable** toolchain.

## Development Workflow

### Before Making Changes

1. Review the project guidelines in `.github/copilot-instructions.md`
2. Understand the profile system (base, desktop) and auto-detected platform categories (linux, windows, arch)
3. Run existing tests:
   ```bash
   cd cli && cargo test
   ```

### Building and Testing Locally

```bash
# Build the binary
cargo build --manifest-path cli/Cargo.toml

# Run all checks (same as CI)
cargo fmt  --check --manifest-path cli/Cargo.toml
cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path cli/Cargo.toml

# Run end-to-end with --build flag (builds from source)
./dotfiles.sh --build install -p base -d
```

### Making Changes

#### Adding Configuration Files

1. **Symlinks**: Add files to `symlinks/` directory, then add entries to `conf/symlinks.ini`:
   ```ini
   [base]
   config/mynewconfig
   ```

2. **Packages**: Add to appropriate section in `conf/packages.ini`:
   ```ini
   [arch]
   my-new-package
   ```

3. **Systemd Units**: Add to `conf/systemd-units.ini`:
   ```ini
   [arch,desktop]
   my-service.service
   ```

4. **File Categorization**: If the file should be excluded in certain profiles, add to `conf/manifest.ini`:
   ```ini
   [desktop]
   symlinks/config/mynewconfig
   ```

#### Adding New Tasks

1. Create a new file in `cli/src/tasks/` implementing the `Task` trait:
   ```rust
   pub struct MyNewTask;

   impl Task for MyNewTask {
       fn name(&self) -> &str { "My New Task" }

       fn should_run(&self, ctx: &Context) -> bool {
           ctx.platform.is_linux()
       }

       fn run(&self, ctx: &Context) -> Result<TaskResult> {
           // Idempotent implementation
           Ok(TaskResult::Ok)
       }
   }
   ```
2. Add the module to `cli/src/tasks/mod.rs`
3. Add the task to the task list in `cli/src/commands/install.rs`

#### Adding New Configuration Types

1. Create INI file in `conf/`
2. Add a parser module in `cli/src/config/` (follow existing patterns like `packages.rs`)
3. Add the field to the `Config` struct and load it in `Config::load()`
4. Create a task in `cli/src/tasks/` that consumes the config

#### Creating New Profiles

Define in `conf/profiles.ini`:
```ini
[my-profile]
include=
exclude=windows,desktop
```

### Rust Code Guidelines

- **Error Handling**: Use `anyhow::Result` with `.context()` for all fallible operations
- **Task Pattern**: Implement the `Task` trait in `cli/src/tasks/`; use `should_run()` for platform/profile gating
- **Idempotency**: Always check if action is needed before taking it
- **No Trailing Whitespace**: Remove all trailing whitespace from files
- **Formatting**: Run `cargo fmt` before committing
- **Linting**: Ensure `cargo clippy -- -D warnings` passes with no warnings

### Shell Wrapper Guidelines

The shell wrappers (`dotfiles.sh`, `dotfiles.ps1`) are thin entry points that download or build the binary. When modifying them:

- **POSIX Compliance**: `dotfiles.sh` uses `#!/bin/sh` — avoid Bash-specific features
- **Minimal Logic**: Keep the wrappers thin; add new logic to the Rust binary instead
- **Error Handling**: `set -o errexit` and `set -o nounset` in `dotfiles.sh`

### INI Configuration Format

See [Configuration Reference](CONFIGURATION.md) and the `ini-configuration` skill for INI format details. Key points:
- Section names use comma-separated categories (logical AND): `[arch,desktop]`
- Exception: `registry.ini` uses `key = value` entries; `profiles.ini` uses hyphenated names

## Testing

### Required Before Submitting

1. **Rust Checks**:
   ```bash
   cargo fmt  --check --manifest-path cli/Cargo.toml
   cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings
   cargo test --manifest-path cli/Cargo.toml
   ```

2. **Dry-Run Testing**:
   ```bash
   ./dotfiles.sh --build install -p desktop -d
   ```
   Verify the binary detects your changes correctly

3. **Profile-Specific Testing**:
   Test with relevant profiles:
   - `base` - Minimal (no desktop tools)
   - `desktop` - Full configuration (includes desktop tools)

   Platform categories (`linux`, `windows`, `arch`) are auto-detected and don't need to be specified.

### CI Testing

GitHub Actions automatically runs on pull requests:
- Rust formatting (`cargo fmt --check`)
- Rust linting (`cargo clippy`)
- Rust tests (`cargo test`)
- Release builds (Linux and Windows)
- Integration tests (dry-run install per profile)
- Shell wrapper linting (shellcheck on `dotfiles.sh` and `install.sh`)
- Docker image build
- Git hooks tests

## Commit Guidelines

### Commit Messages

Follow conventional commit format:

```
type(scope): brief description

Optional longer description explaining the change.

Fixes #issue_number
```

**Types**:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, no logic change)
- `refactor`: Code refactoring
- `test`: Adding or updating tests
- `chore`: Maintenance tasks

**Examples**:
```
feat(symlinks): add neovim configuration
fix(tasks): correct symlink permission check
docs(readme): update installation instructions
chore(ci): update shellcheck version
```

### Commits

- Keep commits atomic (one logical change per commit)
- Write clear, descriptive commit messages
- Reference issues when applicable

## Pull Request Process

1. **Before Submitting**:
   - Ensure all Rust checks pass (`cargo fmt --check`, `cargo clippy`, `cargo test`)
   - Test with `-d` (dry-run) mode
   - Update documentation if needed
   - Remove trailing whitespace from all files

2. **PR Description**:
   - Describe what changes you made and why
   - Reference related issues
   - Include testing notes (which profiles tested)
   - Note any breaking changes

3. **Checklist** (automatically provided by PR template):
   - [ ] Ran `cargo test` successfully
   - [ ] Ran `cargo clippy` with no warnings
   - [ ] Tested with `-d` (dry-run) mode
   - [ ] No trailing whitespace
   - [ ] Updated documentation

4. **Review Process**:
   - Address feedback from maintainers
   - Keep PR focused on single feature/fix
   - Rebase if requested to maintain clean history

## Code Style

See the `rust-patterns` and `shell-patterns` skills in `.github/skills/` for detailed coding conventions. Summary:

- **Rust**: `cargo fmt`, `cargo clippy -- -D warnings`, `anyhow::Result` with `.context()`
- **Shell**: POSIX `#!/bin/sh`, 2-space indent, minimal logic in wrappers
- **PowerShell**: 4-space indent, minimal logic in `dotfiles.ps1`
- **Config files**: One entry per line, comment non-obvious entries

## Documentation Updates

When adding features or changing behavior:

1. Update main [README.md](../README.md) if it affects core usage
2. Update specialized documentation:
   - [USAGE.md](USAGE.md) - For installation and usage changes
   - [PROFILES.md](PROFILES.md) - For profile system changes
   - [CONFIGURATION.md](CONFIGURATION.md) - For configuration file changes
   - [WINDOWS.md](WINDOWS.md) - For Windows-specific changes
   - [ARCHITECTURE.md](ARCHITECTURE.md) - For implementation changes
   - [TROUBLESHOOTING.md](TROUBLESHOOTING.md) - For common issues
3. Add examples to help users understand the feature
4. Update cross-references in related documents
5. Update [docs/README.md](README.md) if adding new documentation files

## Questions or Help

- Open an issue for questions
- Check existing issues and PRs for similar topics
- Review project documentation thoroughly first

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
