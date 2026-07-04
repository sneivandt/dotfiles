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

**Agent Skills**: For technical coding patterns, Copilot and Codex use shared
skills in `.agents/skills/`. See `AGENTS.md` for the complete context map.

## Prerequisites

Install the Rust toolchain via [rustup](https://rustup.rs/):

- **Linux**: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Windows**: `winget install Rustlang.Rustup`

After installation, ensure `cargo` is on your PATH (open a new terminal if needed). The repository pins its Rust version in `rust-toolchain.toml`, and `rustup` will automatically select or install that toolchain when you run `cargo` from this repository.

## Development Workflow

### Before Making Changes

1. Understand the profile system (base, desktop) and auto-detected platform categories (linux, windows, arch)
2. Run existing tests:
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
cargo clippy --manifest-path cli/Cargo.toml --target x86_64-pc-windows-gnu --all-targets -- -D warnings
cargo test --manifest-path cli/Cargo.toml

# Run end-to-end with --build flag (builds from source)
./dotfiles.sh --build install -p base -d
```

### Making Changes

#### Adding Configuration Files

1. **Symlinks**: Add files to `symlinks/` directory, then add entries to `conf/symlinks.toml`:
   ```toml
   [base]
   symlinks = [
     "config/mynewconfig",
     "apm/config/base.yml",
     "apm/plugins/*",
   ]
   ```
   Use a full path-segment `*` when a directory should expand into one symlink per direct child.

2. **Packages**: Add to appropriate section in `conf/packages.toml`:
   ```toml
   [arch]
   packages = ["my-new-package"]
   ```

3. **Systemd Units**: Add to `conf/systemd-units.toml`:
   ```toml
   [arch-desktop]
   units = ["my-service.service"]
   ```

4. **Git Configuration**: Add Windows git settings to `conf/git-config.toml`:
   ```toml
   [windows]
   settings = [
     { key = "core.autocrlf", value = "false" },
     { key = "core.symlinks", value = "true" },
   ]
   ```

5. **File Categorization**: If the file should be excluded in certain profiles, add to `conf/manifest.toml`:
   ```toml
   [desktop]
   paths = ["config/mynewconfig"]
   ```

#### Adding New Tasks

Prefer the existing task macros over hand-written `Task` implementations:

1. Check `.agents/skills/resource-implementation/SKILL.md`,
   `.agents/skills/rust-patterns/SKILL.md`, and the closest existing task for
   the current pattern.
2. Use `resource_task!` for config-backed tasks that process a list of
   resources through the shared resource engine.
3. Use `task_metadata!` for hand-written tasks that need custom control flow
   but still have static name, phase, domain, policies, and dependency
   metadata.
4. Declare dependencies with `task_deps![...]` instead of ad-hoc ordering.
5. Add the module to the domain's `cli/src/tasks/<domain>/mod.rs`.
6. Register the task in `all_install_tasks()` or `all_uninstall_tasks()` in
   `cli/src/tasks/catalog.rs`.
7. Add unit or integration tests for gating, idempotency, and dry-run behavior.

#### Adding New Configuration Types

1. Create TOML file in `conf/`
2. Add a parser module in `cli/src/config/`; prefer `config_section!` for
   sectioned TOML lists
3. Add the field to the `Config` struct and a single `SectionLoader` call in
   `Config::load()`. One call (e.g. `sections.collect_filtered(...)`) loads the
   main config *and* merges the overlay, so there is no separate merge step.
4. Create a task in the appropriate `cli/src/tasks/<domain>/` folder that consumes the config

#### Creating New Profiles

Define in `conf/profiles.toml`:
```toml
[my-profile]
include = []
exclude = ["windows", "desktop"]
```

### Rust Code Guidelines

- **Error Handling**: Use `anyhow::Result` with `.context()` for all fallible operations
- **Task Pattern**: Prefer `resource_task!` for resource-backed tasks and `task_metadata!` for custom tasks; use `should_run()` only for platform/profile gating that cannot be represented by execution policies
- **Idempotency**: Always check if action is needed before taking it
- **No Trailing Whitespace**: Remove all trailing whitespace from files
- **Formatting**: Run `cargo fmt` before committing
- **Linting**: Ensure host and Windows-target `cargo clippy --all-targets -- -D warnings` pass with no warnings. `cli/Cargo.toml` is the source of truth for strict lints, including bans on silent `as` conversions, ambiguous ref-counted pointer clones, wildcard enum arms, unrelated shadowing, and ignored `#[must_use]` results
- **Lint Allows**: Every `#[allow(...)]` must include a `reason = "..."`
  argument; avoid `.unwrap()` and `.expect()` in production code

### Shell Wrapper Guidelines

The shell wrappers (`dotfiles.sh`, `dotfiles.ps1`) are thin entry points that download or build the binary. When modifying them:

- **POSIX Compliance**: `dotfiles.sh` uses `#!/bin/sh` — avoid Bash-specific features
- **Minimal Logic**: Keep the wrappers thin; add new logic to the Rust binary instead
- **Error Handling**: `set -o errexit` and `set -o nounset` in `dotfiles.sh`

### TOML Configuration Format

See [Configuration Reference](CONFIGURATION.md) and the `toml-configuration` skill for TOML format details. Key points:
- Section names use hyphen-separated categories (logical AND): `[arch-desktop]`
- `registry.toml` uses logical section names with `path` key and nested `[section.values]` subtable; `profiles.toml` uses profile names

## Testing

### Required Before Submitting

1. **Rust Checks**:
   ```bash
   cargo fmt  --check --manifest-path cli/Cargo.toml
   cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings
   cargo clippy --manifest-path cli/Cargo.toml --target x86_64-pc-windows-gnu --all-targets -- -D warnings
   cargo test --manifest-path cli/Cargo.toml
   ```
   Install the Windows target first when needed:
   `rustup target add x86_64-pc-windows-gnu` plus a mingw-w64 GCC toolchain.

   For changes under `conf/`, `symlinks/`, `hooks/`, or wrapper scripts, also
   run:
   ```bash
   ./dotfiles.sh test
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
- Security audit (`cargo audit`, `cargo deny`)
- Release builds (Linux and Windows)
- Integration tests (dry-run install per profile)
- Install/uninstall round-trip tests
- Shell wrapper linting (shellcheck on `dotfiles.sh` and `install.sh`)
- Application tests (git, zsh, vim, nvim)
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
   - Ensure all Rust checks pass (`cargo fmt --check`, host Clippy,
     Windows-target Clippy, and `cargo test`)
   - Run `./dotfiles.sh test` for configuration, symlink, hook, or wrapper changes
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
   - [ ] Ran Windows-target Clippy for Rust changes
   - [ ] Ran `./dotfiles.sh test` for configuration/symlink/hook/wrapper changes
   - [ ] Tested with `-d` (dry-run) mode
   - [ ] No trailing whitespace
   - [ ] Updated documentation

4. **Review Process**:
   - Address feedback from maintainers
   - Keep PR focused on single feature/fix
   - Rebase if requested to maintain clean history

## Code Style

See the `rust-patterns` and `shell-patterns` skills in `.agents/skills/` for
detailed coding conventions. Summary:

- **Rust**: `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `anyhow::Result` with `.context()`
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
5. Update the [documentation index](README.md) if adding new documentation files

## Questions or Help

- Open an issue for questions
- Check existing issues and PRs for similar topics
- Review project documentation thoroughly first

## Next read

- [Testing](TESTING.md) - Exact local and CI validation commands
- [Architecture](ARCHITECTURE.md) - How tasks, resources, and scheduling fit together
- [Configuration Reference](CONFIGURATION.md) - TOML formats and cookbook examples

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
