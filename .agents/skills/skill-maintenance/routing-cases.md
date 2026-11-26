# Skill routing review cases

Manual boundary cases for checking trigger descriptions before loading skill
bodies. Keep only ambiguous neighboring tasks and real cross-domain boundaries;
see [Testing](../../../docs/content/development/testing.md#choosing-coverage)
for local coverage selection and
[Writing documentation](../../../docs/content/development/writing-documentation.md#acceptance-scenarios)
for documentation acceptance cases.

| Request | Primary skill | Boundary or companion |
|---|---|---|
| Preserve an empty argument forwarded by `dotfiles.sh` | `self-update` | Wrapper bootstrap section; no `shell-configuration` |
| Fix duplicate `PATH` entries when sourcing Zsh startup twice | `shell-configuration` | No `self-update` or `engine-orchestration` |
| Add a managed shell config link for desktop users | `symlink-management` | Add `shell-configuration` only if startup content changes |
| Change Hyprland monitor rules or locker service sandbox | `hyprland-session` | Add `desktop-shell` only if Quickshell UI changes |
| Stop Quickshell polling Wi-Fi while the network menu is closed | `desktop-shell` | Add `hyprland-session` only if session/service integration changes |
| Configure a Neovim plugin or lazy.nvim bootstrap pin | `editor-configuration` | No Rust engine skill |
| Add a VS Code extension `Resource` | `engine-orchestration` | Resources section; no `editor-configuration` |
| Change release asset names and their self-update/wrapper consumers | `ci-cd-patterns` | Add `self-update` for Rust and wrapper consumers |
| Change Git repository sync before task restart | `engine-orchestration` | No `self-update` (binary self-update only) |
| Change failed-command stream retention or `.unchecked()` exit handling | `error-handling-patterns` | Add `logging-patterns` only when console/log rendering changes |
| Change `dotfiles update` spacing and refresh `terminal-screenshot.svg` | `logging-patterns` | Add `docs-publishing` for site asset/browser assertions |
| Add a dotfiles-managed PKGBUILD snapshot input | `arch-package-recipes` | No `package-management` unless provider/selection changes |
| Add an ordinary `conf/packages.toml` entry | `package-management` | No `arch-package-recipes` unless packaging changes |
| Change APM fragment target deployment | `agent-tooling` | No `arch-package-recipes` unless the APM PKGBUILD changes |
| Add a field to `conf/agent-settings.toml` with semantic diagnostics | `agent-tooling` | Add `toml-configuration` for schema/validation/drift rules |
| Change an optional overlay section's generic TOML loader or profile category | `toml-configuration` | Add `overlay-scripts` only for overlay path resolution or script execution |
| Investigate a failed Mutation Testing workflow run or exclusion | `mutation-testing` | Add `ci-cd-patterns` only for workflow permissions or action pins |
| Change `mutation.yml` permissions or action pins | `ci-cd-patterns` | Add `mutation-testing` only if selection/exclusion semantics change |
| Monitor CI after pushing a repo-local skill wording change | `ci-cd-patterns` | Exact-SHA links/metadata checks (`pages.yml` skips site build/deploy for skill-only diffs; APM skills under `symlinks/apm/` route through config-drift/APM, not site docs) |
| Rewrite a guide or task reference page for accuracy | `docs-writing` | Add `docs-publishing` only for routes, nav, assets, or site checks |
| Change VitePress navigation, theme CSS, assets, or docs domain | `docs-publishing` | Add `docs-writing` only when rewriting prose |
| Edit `.agents/skills/` or `AGENTS.md` without editing human guides | `skill-maintenance` | Neither docs skill merely because the file is Markdown |
