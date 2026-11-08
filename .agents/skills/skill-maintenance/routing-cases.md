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
| Monitor checks after pushing a skill wording change | `ci-cd-patterns` | Follow exact-SHA CI and Pages; do not rerun local suites while waiting |
| Select tests before committing an isolated Rust resource change | [Testing guide](../../../docs/content/development/testing.md#choosing-coverage) | Affected targets and host checks; no separate verification skill |
| Choose Windows coverage after a Rust path-handling change | [Testing guide](../../../docs/content/development/testing.md#platform-coverage-parity) | Platform coverage parity; windows-specific-patterns only for implementation choices |
| Enable dark mode and fix Mermaid on the published docs site | `docs-publishing` | No desktop-shell skill |
| Move site-maintenance instructions into docs/README.md | `docs-publishing` | Add docs-writing for prose; keep the README outside published routes |
| Audit the guides for accuracy, readability, and excessive detail | `docs-writing` | Verify claims against sources; add docs-publishing only for site or organization changes |
| Explain switching an installed Git link to a private overlay | `docs-writing` | Review the starting state and link conflict, not just the declaration's merge behavior |
| Improve instructions for retiring desktop links and returning to base | `docs-writing` | Materialized base copies must not be described as automatically replaceable by links |
| Expand the Systemd units task page with domain context and upstream references | `docs-writing` | Load its task-reference checklist to verify the task, resource, and catalog |
| Improve the Getting started walkthrough without changing task pages | `docs-writing` | No task-reference checklist for an ordinary workflow guide |
| Add a task reference page and nest it beneath the task overview | `docs-writing` | Load its task-reference checklist; add docs-publishing for navigation and route coverage |
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

When reviewing documentation guidance, use the human guide's
[acceptance scenarios](../../../docs/content/development/writing-documentation.md#acceptance-scenarios)
as well. Check the explanation's essential facts, not a fixed wording.

## Rust ownership review cases

| Request | Primary skill | Boundary or companion |
|---|---|---|
| Change shared scheduler failure propagation | `engine-orchestration` | Inspect affected command contracts; no preliminary Rust router |
| Add a VS Code extension resource | `resource-implementation` | Engine guidance only if scheduling changes |
| Add a TOML field with a semantic diagnostic | `toml-configuration` | Add config-validation for the diagnostic contract |

## Local coverage review cases

Walk these diffs through the verification guidance without executing unrelated
suites. The result should explain why a check can exercise the change.

| Diff | Expected local choice | Remote follow-through |
|---|---|---|
| Skill wording and ordinary guide prose | Source review, affected links and skill metadata; no npm, site, or Rust | Exact-SHA aggregate, docs, and selected publication |
| Heading rename or moved page | Incoming links; build for moved routes and generated navigation | Full docs job and Pages |
| Search hook, theme CSS, or Mermaid renderer | Affected Node/type checks, build, focused browser cases | Full docs/browser suite and Pages |
| Linux-only resource implementation | fmt, host Clippy, owning tests; no automatic Windows compile | Selected native platform and mutation jobs |
| Shared path helper or platform gates | Owning tests plus affected-target compilation; native runtime where available | Native Windows path behavior and selected mutation reports |
| Shared scheduler or loader | Broader Rust suite and affected command contracts | Full selected platform/integration matrix |
| One integration-test assertion | Its target; verify the filter ran tests | Selected CI tests, retaining pending source range after a failed push |

A passing focused check need not be repeated merely because a commit was
created. Verify the committed inputs match; rerun after relevant edits or
uncertain provenance. A local failure still needs resolution. CI deferral is
not a local pass, and a successful push does not end the task while CI is pending.
