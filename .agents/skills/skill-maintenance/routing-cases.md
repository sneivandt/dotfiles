# Skill routing review cases

Use these cases when editing descriptions. Compare the request with the actual
frontmatter before loading bodies, then inspect the selected procedure. These
are manual acceptance cases, not an automated model-selection benchmark.
Companions are needed only when the request changes their contract.

| Request | Primary skill | Boundary or companion |
|---|---|---|
| Fix duplicate PATH entries when sourcing Zsh startup twice | `shell-configuration` | No bootstrap or engine skill |
| Preserve an empty argument forwarded by dotfiles.sh | `shell-patterns` | No shell-configuration skill |
| Add a managed shell config link for desktop users | `symlink-management` | Add shell-configuration only if startup content changes |
| Fix an interactive Zsh prompt without changing profile selection | `shell-configuration` | No profile-system skill |
| Change Hyprland monitor rules | `hyprland-session` | No Quickshell skill unless its UI changes |
| Keep Hypridle and its locker working under the user-service sandbox | `hyprland-session` | Inspect service and helper together |
| Fix calendar popup focus in QML | `desktop-shell` | Add hyprland-session only if session/service integration changes |
| Configure a Neovim plugin without changing plain Vim startup | `editor-configuration` | No Rust resource skill |
| Change lazy.nvim's bootstrap revision | `editor-configuration` | Review only the matching pin and lock entry; use isolated first launch |
| Add a VS Code extension resource | `resource-implementation` | No editor-configuration skill |
| Preserve the old binary after a failed self-update smoke test | `self-update` | Add error-handling-patterns if error semantics change |
| Change release asset names and their self-update consumer | `ci-cd-patterns` | Add self-update and shell-patterns for affected consumers |
| Change Git repository synchronization before task restart | `engine-orchestration` | Inspect repository domain; no binary self-update skill |
| Change failed-command stream retention | `error-handling-patterns` | Add logging-patterns only when rendering/persistence changes |
| Change a task row's ANSI style | `logging-patterns` | No generic error-handling skill |
| Add a dotfiles-managed PKGBUILD snapshot input | `arch-package-recipes` | Extend documented format and cleanup fixtures |
| Add an ordinary conf/packages.toml entry | `package-management` | No arch-package-recipes skill unless packaging changes |
| Change APM fragment target deployment | `agent-tooling` | No arch-package-recipes skill unless the APM executable recipe changes |
| Change an optional overlay section's generic TOML loader | `toml-configuration` | Add overlay-scripts only for resolution or execution changes |
| Choose Windows coverage after a Rust path-handling change | `cross-platform-verification` | windows-specific-patterns owns implementation choices |
| Enable dark mode and fix Mermaid on the published docs site | `docs-publishing` | No desktop-shell skill |
| Audit the guides for accuracy, readability, and excessive detail | `docs-writing` | Verify claims against sources; add docs-publishing only for site or organization changes |
| Simplify examples in Commands and usage | `docs-writing` | Verify defaults in the owning code; no engine changes just for prose |
| Make each guide understandable when shared directly | `docs-writing` | Supply task-local context; add docs-publishing only for page or navigation changes |
| Reduce boxed text and balance links versus bold in the guides | `docs-writing` | Preserve exact syntax and safety warnings; no CSS workaround |
| Change the colors of inline code without rewriting prose | `docs-publishing` | Rendering only, not a docs-writing task |
| Remove em dashes and formulaic wording from the README | `docs-writing` | Preserve exact commands, quoted output, and meaningful technical punctuation |
| Move desktop documentation into a subsystem guide | `docs-publishing` | Add docs-writing when rewriting prose; preserve one explanation, matching title/file/URL, and incoming references |
| Explain a changed self-update behavior to users | `docs-writing` | Add self-update only for implementation work or a behavioral question it owns |
| Change an internal agent instruction without editing human guides | `skill-maintenance` | Neither docs skill is needed merely because the instruction is Markdown |
| Change the documentation domain without rewriting guide prose | `docs-publishing` | Keep homepage metadata, robots, README links, and browser assertions aligned; no docs-writing skill |
| Replace the Pages build with a custom Actions workflow | `docs-publishing` | Add ci-cd-patterns for workflow permissions, pins, and guards |
| Review skill triggers without changing behavior | `skill-maintenance` | No engine skills merely because examples mention tasks |

For each new or revised trigger, add a matching request, a neighboring exclusion,
and a request crossing its real boundary. Check that the body points to current
sources and useful fixture coverage. Record uncertainty rather than claiming
these examples prove runtime routing quality.
