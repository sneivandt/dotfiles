# Pull Request

## Summary
<!-- What changed, and why is this change needed? -->

## Related Issues
<!-- Use "Fixes #123", link related issues, or write "N/A". -->

## Change Type
<!-- Check all that apply. -->
- [ ] Feature
- [ ] Bug fix
- [ ] Configuration or dotfiles update
- [ ] Refactoring
- [ ] Tests
- [ ] Documentation
- [ ] CI, build, or dependency maintenance

## Scope
<!-- Check all affected profiles and platform categories. -->
- [ ] `base`
- [ ] `desktop`
- [ ] `linux`
- [ ] `arch`
- [ ] `windows`
- [ ] Profile/platform agnostic

## Validation
<!-- Check only commands or behaviors you verified. Explain omitted relevant checks below. -->

### Rust changes
- [ ] `cargo fmt --check --manifest-path cli/Cargo.toml`
- [ ] `cargo clippy --profile ci --manifest-path cli/Cargo.toml --all-targets -- -D warnings`
- [ ] `cargo clippy --profile ci --manifest-path cli/Cargo.toml --target x86_64-pc-windows-gnu --all-targets -- -D warnings`
- [ ] `cargo test --profile ci --manifest-path cli/Cargo.toml`

### Configuration, wrappers, hooks, and behavior
- [ ] Ran config validation from source for the affected platform/profile
      (`./dotfiles.sh --build -p base test` or `.\dotfiles.ps1 --build -p base test`)
- [ ] Ran a source-built dry-run for each affected profile
      (`./dotfiles.sh --build install -p <profile> -d` or Windows equivalent)
- [ ] Verified idempotency when the change mutates machine state

**Test environment**
- OS: <!-- e.g. Arch Linux, Ubuntu, Windows 11 -->
- Profiles: <!-- e.g. base, desktop, or N/A -->
- Checks not run: <!-- Explain why, or write "None". -->

## User-Facing Impact
- Configuration or manifest changes: <!-- Summarize, or write "None". -->
- Breaking changes or migration steps: <!-- Summarize, or write "None". -->
- Documentation updated: <!-- Link files, or explain why no update is needed. -->

## Final Checklist
- [ ] The change is focused and contains no unrelated files
- [ ] Mutations are idempotent and dry-run safe, or this is not applicable
- [ ] Conditional symlink and manifest coverage remain synchronized, or this is not applicable
- [ ] No private files, credentials, or other secrets are included
- [ ] Relevant documentation and tests are updated, or no update is needed

## Review Notes
<!-- Call out risks, tradeoffs, follow-up work, or areas needing extra attention. -->
