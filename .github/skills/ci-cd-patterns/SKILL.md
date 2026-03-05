---
name: ci-cd-patterns
description: >
  CI/CD pipeline structure, release workflow, and integration test scripts.
  Use when modifying GitHub Actions workflows, adding CI jobs, or changing
  the release/binary distribution process.
metadata:
  author: sneivandt
  version: "1.0"
---

# CI/CD Patterns

Three GitHub Actions workflows in `.github/workflows/`:

| Workflow | Trigger | Purpose |
|---|---|---|
| `ci.yml` | Push/PR to master | Build, lint, test, integration checks |
| `release.yml` | Push to master (cli/conf paths) | Build release binaries, create GitHub Release |
| `docker-image.yml` | Push to master | Build and push Docker image to Docker Hub |

## CI Pipeline (`ci.yml`)

### Job Structure

```
rust-fmt ─────────────────────────────────────────────┐
lint (ShellCheck, PSScriptAnalyzer) ──────────────────┤
validate-config ──────────────────────────────────────┤
audit (cargo-audit) ──────────────────────────────────┤
build-linux ──┬── integration-linux (base, desktop) ──┤
              ├── test-install-uninstall ──────────────┤
              ├── test-applications (git, zsh, vim…) ──┤
              ├── test-git-hooks ──────────────────────┤
              └── test-shell-wrapper-linux ────────────┤
build-windows ┬── integration-windows (base, desktop) ┤
              ├── test-install-uninstall-windows ──────┤
              └── test-shell-wrapper-windows ──────────┤
ci-success (gate) ────────────────────────────────────┘
```

Key patterns:
- `concurrency: ci-${{ github.ref }}` with `cancel-in-progress: true`
- `permissions: contents: read` (least privilege)
- Uses `--profile ci` for faster builds (optimised dev profile)
- Build artifacts uploaded with 1-day retention for downstream jobs
- `ci-success` gate job uses `if: always()` with failure/cancelled check

### Build Profiles

CI uses `cargo build --profile ci` (not `--release`) for faster compilation
while still catching release-mode issues. The release workflow uses
`cargo build --release`.

### Test Scripts

Integration test logic lives in `.github/workflows/scripts/`:

```
scripts/
├── linux/
│   ├── lib/test-helpers.sh     # Shared test helpers
│   ├── test-applications.sh    # App-specific tests (git, zsh, vim, nvim)
│   ├── test-config.sh          # Config validation checks
│   ├── test-git-hooks.sh       # Pre-commit hook tests
│   ├── test-shell-wrapper.sh   # dotfiles.sh wrapper tests
│   ├── test-static-analysis.sh # ShellCheck/PSScriptAnalyzer runners
│   └── test-uninstall.sh       # Install/uninstall round-trip
└── windows/
    ├── Test-ShellWrapper.ps1   # dotfiles.ps1 wrapper tests
    └── Test-InstallUninstall.ps1
```

### Integration Test Strategy

- **Dry-run profile tests**: Run `bin/dotfiles --root . -p <profile> -d install` for
  each profile on both Linux and Windows
- **Config validation**: Run `bin/dotfiles --root . -p <profile> test`
- **Install/uninstall round-trip**: Install then uninstall, verify cleanup
- **Application tests**: Install with base profile, then test each app (git config,
  zsh completion, vim/nvim open and plugins)

## Release Pipeline (`release.yml`)

1. Builds release binaries on both Linux and Windows
2. Sets `DOTFILES_VERSION=v0.1.${{ github.run_number }}` as env var
3. `build.rs` embeds this version via `cargo:rustc-env=DOTFILES_VERSION`
4. Strips the Linux binary (`strip cli/target/release/dotfiles`)
5. Renames to `dotfiles-linux-x86_64` / `dotfiles-windows-x86_64.exe`
6. Generates `checksums.sha256` via `sha256sum`
7. Creates a GitHub Release with `softprops/action-gh-release@v2`

The shell wrappers (`dotfiles.sh`, `dotfiles.ps1`) download these release
binaries, verify the SHA256 checksum, and cache the installed version.

## Adding a New CI Job

1. Add the job to `ci.yml` with appropriate `needs:` dependencies
2. Add the job name to the `ci-success` gate's `needs:` list
3. Use `fail-fast: false` in matrix strategies for independent test cases
4. Use the shared test helper library in `scripts/linux/lib/test-helpers.sh`
5. Download build artifacts when tests need the compiled binary

## Rules

- All CI jobs must be listed in the `ci-success` gate's `needs:` array
- Use `--profile ci` in CI builds, `--release` only in release workflow
- Test scripts go in `.github/workflows/scripts/` — not inline in YAML
- Integration tests use `--root .` to run against the checked-out repo
- Release version comes from `github.run_number` — no manual tagging
- Binary checksums are always generated and published with releases
