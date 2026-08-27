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
| APM packages task | Merges fragments, persists the generated manifest, and invokes native APM convergence |
| APM package updates task | Invokes native APM update during `dotfiles install --update-pins` |
| APM itself | Resolves packages, verifies local sources, converges deployments, and removes stale content |

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
is already current, the primary native install still runs. After APM finishes,
the task restores `autopilot` mode and enabled state for any dotfiles-managed
workflow that drifted.

Cowork remains an experimental APM target and is disabled by default. When a
Cowork skills path is available, dotfiles re-asserts the feature with:

```bash
apm experimental enable copilot-cowork
```

Dotfiles follows APM's Cowork path precedence: the
`APM_COPILOT_COWORK_SKILLS_DIR` environment variable, the persisted
`copilot_cowork_skills_dir` value in `~/.apm/config.json`, then the
`ONEDRIVECOMMERCIAL` or `ONEDRIVE` Windows fallback. A configured path also
enables Cowork reconciliation on Linux.

Current APM still replaces a colliding skill directory with directory removal
followed by a full tree copy. Cowork stores skills under OneDrive and protects
those directories, so dotfiles must not invoke the native `copilot-cowork`
target yet. Instead, after the primary install, it reads each locked
dependency's `target_subset`, selects packages with no filter or a filter
containing `copilot-cowork`, and copies their resolved skills from
`~/.agents/skills` file-by-file. It removes `SKILL.md` from excluded or removed
skills but preserves Cowork-owned placeholders, directories, and ACLs.

Reconciliation removes legacy `cowork://` deployment records before native APM
convergence. Otherwise, unrelated APM commands can retry directory deletion
that OneDrive blocks. Dry-run reports the planned file-level reconciliation
without modifying Cowork or the lockfile.

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
3. Writes the generated manifest only when its content changed.
4. Runs `apm install -g` on every applicable install pass.
5. Lets APM verify local sources, converge deployments, and remove stale or
   orphaned user-scope content.
6. Compares the exact lockfile before and after to report whether APM changed
   resolved state.

Re-running `dotfiles install` should not advance pinned dependency versions.
Native APM owns idempotency through its lockfile. **APM packages** can therefore
invoke APM every time while still reporting a current task when the generated
manifest, lockfile, and retained autopilot policy are unchanged. Dry-run
previews the delegated install and target work without writing the generated
manifest.

```bash
dotfiles install --only apm --dry-run --verbose
dotfiles install --only apm
```

## Pin-update behavior

**APM package updates** is marked update-only, so it runs with
`dotfiles install --update-pins` but not ordinary `dotfiles install`. It depends on
**APM packages**.

The install dependency first converges the generated manifest. Apply then runs
`apm update -g --yes` directly; dry-run uses APM's native
`apm update -g --dry-run` plan. No separate `apm outdated` parser or dotfiles
success marker is involved.

The task compares the exact lockfile bytes before and after update. Current APM
preserves unchanged target mappings and timestamps, so an identical lockfile
reports current while any native lock-state change is reported as changed.

```bash
dotfiles install --update-pins --only apm,apm-update
```

## Overlays

Private overlays can contribute additional APM fragments and local plugins.
The merged configuration appends overlay content rather than replacing the main
repository's declarations. Keep private package locations and agent-specific
configuration out of the public repository.

Validate combined public and overlay state using
[CLI validation](TESTING.md#cli-validation), then preview APM reconciliation as
described in [Dry-run testing](TESTING.md#dry-run-testing).

## Validation

**Validate config warnings** checks the dotfiles-specific cross-file invariant
that each local `~/.apm/plugins/dot-*` reference has a matching source directory
in the same repository or overlay. Native APM owns YAML syntax, dependency
fields, MCP declarations, and package-layout validation. **Validate APM
plugins** runs `apm pack --dry-run --verbose` for every local plugin directory.
If APM is not installed, the pack check is reported as unavailable.

When changing APM configuration, check:

- native APM validation succeeds for the fragments and packages
- deterministic merged ordering
- symlink and sparse-manifest alignment
- local plugin paths that exist in the selected checkout
- native install/update dry-run behavior

## Adding an APM package

1. Choose the narrowest applicable fragment in `symlinks/apm/config/`.
2. Add a pinned or policy-compliant package declaration.
3. If the fragment is conditional, confirm its symlink and manifest categories.
4. Follow the APM coverage in [Testing](TESTING.md).
5. Run install before using `dotfiles install --update-pins` to advance versions.

Represent changes in a source fragment. Do not edit generated merged state or
lock data by hand.
