# APM and AI tooling

The repository uses [APM](https://github.com/microsoft/apm) to distribute
skills, plugins, instructions, hooks, and MCP configuration to supported AI
agents. Dotfiles defines the desired state. APM resolves packages and writes
their content.

## Responsibilities

| Layer | Responsibility |
|---|---|
| `conf/agent-settings.toml` | Converges stable per-harness preferences in Copilot JSON and Codex TOML |
| `symlinks/apm/config/*.yml` | Profile-specific APM source fragments |
| `conf/symlinks.toml` | Selects and links applicable fragments and local plugins |
| `conf/manifest.toml` | Removes inapplicable platform fragments from sparse checkout |
| APM packages task | Merges fragments and converges installed state |
| APM package updates task | Advances eligible pinned versions during `dotfiles update` |
| APM itself | Resolves packages and distributes their content |

Use APM to place APM-managed content in agent directories. Do not maintain
separate copies.

APM owns distributable content such as skills, plugins, hooks, instructions,
and MCP declarations. `conf/agent-settings.toml` owns selected harness
preferences such as model, reasoning effort, and terminal UI options.
Codex settings in `~/.codex/config.toml` are shared by its CLI, IDE extension,
and agent inside the ChatGPT desktop app; app-only preferences remain in the
desktop app.

## Configuration fragments

The source fragments are stored under:

```text
symlinks\apm\config\
```

The active profile controls which fragments remain in the checkout and which
ones are linked. The task merges main and private-overlay fragments into one
generated desired state. Put platform-specific packages in the matching profile
fragment, not behind runtime conditions in generated output.

Local plugin sources live under:

```text
symlinks\apm\plugins\
```

Managed symlinks expose these plugins to APM without requiring a published
package.

## Deployment targets

`symlinks/apm/config/base.yml` declares the cross-platform `targets:` list:

```yaml
targets:
  - agent-skills
  - copilot
```

Without this list, APM detects every agent harness directory in the home
directory and deploys to each one. That can write skills and MCP declarations
to unused runtimes. The explicit list limits deployment to configured runtimes.
At the next convergence, APM removes its content from runtimes removed from the
list.

APM resolves the runtime set in this order:

1. An explicit `--target` flag on the invocation.
2. The `targets:` list in the merged manifest.
3. `apm config target`.
4. Auto-detection.

The APM packages task omits `--target` from its primary invocation, leaving the
merged manifest in control. Copilot App is the exception because APM does not
accept its experimental target in `apm.yml`. When the App database exists, the
task idempotently enables `copilot-app` and runs a separate
`apm install -g --target copilot-app` to deploy workflows. If the APM manifest
is already current, the task still checks dotfiles-managed workflow rows. It
restores `autopilot` mode and enabled state for any workflow that drifted,
without rerunning APM.

Cowork stores skills under OneDrive and prevents deletion of existing skill
directories. APM replaces colliding directories as a unit, so repeated direct
`copilot-cowork` installs fail with access denied after Cowork creates those
directories. On Windows, the task uses a file-level process instead. After the
primary install, it reads each locked dependency's `target_subset`, selects
packages with no filter or a filter containing `copilot-cowork`, and copies
their resolved skills from `~/.agents/skills`. It removes `SKILL.md` from
excluded or removed skills but preserves Cowork-owned placeholders,
directories, and ACLs.

Reconciliation also removes legacy `cowork://` deployment records from older
direct installs. Otherwise, unrelated APM commands would retry directory
deletion that OneDrive blocks. Dry-run compares files and ledger state, so it
reports missing, changed, or incorrectly included Cowork skills before apply
uses the same reconciliation.

Fragments merge their `targets:` lists by union with deduplication, so a private
overlay fragment can add a runtime without restating the base list.

## Install behavior

**APM packages** depends on:

- regular packages
- AUR packages
- symlinks

This order makes the APM executable and repository-managed fragments available
before convergence. The task:

1. Discovers active main and overlay fragments from their configured symlink
   sources, while preserving unmanaged home fragments.
2. Produces the merged manifest in deterministic order.
3. Computes a fingerprint of the merged desired state.
4. Runs APM's idempotent convergence.
5. Records the successful fingerprint for update safety.
6. Prunes user-scope deployments no longer owned by the generated manifest.

Re-running `dotfiles install` should not advance pinned dependency versions.
Install previews derive the same post-symlink merged manifest, lockfile, and
success-marker state as apply mode. A missing managed fragment link can
therefore be previewed and restored by **Home symlinks** without incorrectly
previewing an APM reinstall. **APM packages** emits `~` only when convergence is
needed. A current install stays quiet.

```bash
dotfiles install --only apm --dry-run --verbose
dotfiles install --only apm
```

## Update behavior

**APM package updates** is marked update-only, so it runs with
`dotfiles update` but not `dotfiles install`. It depends on
**APM packages**.

Before advancing versions, the task verifies that installed state matches the
current merged-manifest fingerprint. It skips the update if install convergence
failed or desired state changed afterward. This avoids changing an unrelated or
partial lockfile.

Preview and apply both run the non-mutating `apm outdated -g` command. Current
dependencies stay quiet. An unrecognized result appears in the preview instead
of being treated as current. When updates exist, apply invokes APM's native
update and compares dependency-resolution state before and after. Volatile
timestamps and deployment or MCP ledgers rewritten during convergence do not
count as version changes.

```bash
dotfiles update --only apm,apm-update
```

## Overlays

Private overlays can contribute additional APM fragments and local plugins.
The merged configuration appends overlay content rather than replacing the main
repository's declarations. Keep private package locations and agent-specific
configuration out of the public repository.

Validate the combined setup:

```bash
dotfiles test --overlay C:\Code\private-dotfiles
dotfiles install --overlay C:\Code\private-dotfiles --only apm --dry-run
```

## Validation

**Validate config warnings** parses main and overlay fragments and checks local
plugin references, Git dependency fields, and MCP entries. **Validate APM
plugins** runs `apm pack --dry-run --verbose` for every local plugin directory.
If APM is not installed, only the pack check is reported as unavailable.

When changing APM configuration, check:

- valid YAML fragments
- deterministic merged ordering
- symlink and sparse-manifest alignment
- local plugin paths that exist in the selected checkout
- install-before-update fingerprint safety

## Adding an APM package

1. Choose the narrowest applicable fragment in `symlinks/apm/config/`.
2. Add a pinned or policy-compliant package declaration.
3. If the fragment is conditional, confirm its symlink and manifest categories.
4. Run `dotfiles test`.
5. Preview with `dotfiles install --only apm --dry-run`.
6. Run install before using `dotfiles update` to advance versions.

Represent changes in a source fragment. Do not edit generated merged state or
lock data by hand.
