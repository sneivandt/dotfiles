# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- New GitHub Actions test workflow with ShellCheck validation
- Test matrix for ubuntu-latest and ubuntu-20.04
- Dependabot configuration for automated dependency updates
- Docker layer caching for faster CI builds
- Hadolint validation for Dockerfile
- Comprehensive architecture documentation (`docs/ARCHITECTURE.md`)
- Contributing guidelines (`docs/CONTRIBUTING.md`)
- Performance optimizations with program cache and progress indicators
- `--quiet` flag for minimal output in scripting contexts
- Execution timing information
- Development and Performance sections in README

### Changed
- Updated `actions/checkout` from v3 to v4
- Updated `docker/login-action` from SHA to version tag v3
- Updated `docker/metadata-action` from SHA to version tag v5
- Updated `docker/build-push-action` from SHA to version tag v5
- Improved README with test workflow badge and troubleshooting section
- Optimized `is_env_ignored` with early returns
- Enhanced batch file operations to reduce redundant operations

### Fixed
- None

### Security
- None

## [1.0.0] - [Date TBD]

Initial release.

[Unreleased]: https://github.com/sneivandt/dotfiles/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/sneivandt/dotfiles/releases/tag/v1.0.0
