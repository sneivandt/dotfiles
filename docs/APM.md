# APM and AI Tooling

The repository uses [APM](https://github.com/microsoft/apm) to distribute
shared skills, plugins, instructions, hooks, and MCP configuration across
supported AI agents. Dotfiles owns the desired state; APM owns package
resolution and materialization.

## Responsibilities

| Layer | Responsibility |
|---|---|
| `symlinks/apm/config/*.yml` | Profile-specific APM source fragments |
| `conf/symlinks.toml` | Selects and links applicable fragments and local plugins |
| `conf/manifest.toml` | Removes inapplicable platform fragments from sparse checkout |
| APM packages task | Merges fragments and converges installed state |
| APM package updates task | Advances eligible pinned versions during `dotfiles update` |
| APM itself | Resolves packages and distributes their content |

Agent directories should generally receive APM-managed content through APM
rather than ad hoc copies.

## Configuration fragments

The source fragments are stored under:

```text
symlinks\apm\config\
```

The active profile controls which fragments are present and linked. Main and
private-overlay fragments are merged into one generated desired state. Keep
platform-specific package declarations in their matching profile fragment
instead of placing runtime conditionals in generated output.

Local plugin sources live under:

```text
symlinks\apm\plugins\
```

They are linked as ordinary managed sources, making local plugin development
available without publishing a package.

## Deployment targets

`symlinks/apm/config/base.yml` declares a top-level `targets:` list that pins
which runtimes APM is allowed to deploy into:

```yaml
targets:
  - agent-skills
  - copilot
```

Without it APM auto-detects every agent harness directory present in the home
directory and deploys there, which writes skills and MCP server declarations
into runtimes that are not actually used. Declaring the list keeps deployment
confined to the runtimes this configuration targets, and APM removes content
from runtimes dropped out of the list on the next convergence.

APM resolves the runtime set in this order:

1. An explicit `--target` flag on the invocation.
2. The `targets:` list in the merged manifest.
3. `apm config target`.
4. Auto-detection.

The APM packages task deliberately omits `--target` on its primary invocation so
the manifest stays the source of truth. `copilot-app` is the exception: it is
experimental and is deployed by a dedicated
`apm install -g --target copilot-app` pass gated on the Copilot App database, so
it is intentionally absent from `targets:`.

Fragments merge their `targets:` lists by union with deduplication, so a private
overlay fragment can add a runtime without restating the base list.

## Install behavior

**APM packages** depends on:

- regular packages
- AUR packages
- symlinks

That ordering ensures the APM executable and repository-managed fragments are
available. The task:

1. Discovers active main and overlay fragments.
2. Produces the merged manifest in deterministic order.
3. Computes a fingerprint of the merged desired state.
4. Runs APM's idempotent convergence.
5. Records the successful fingerprint for update safety.
6. Prunes user-scope deployments no longer owned by the generated manifest.

Re-running `dotfiles install` should not advance pinned dependency versions.
Install previews derive the same merged-manifest, lockfile, and success-marker
state as apply mode. They emit `~` only when convergence is needed; an
already-current install stays quiet.

```bash
dotfiles install --only apm --dry-run --verbose
dotfiles install --only apm
```

## Update behavior

**APM package updates** is marked update-only, so it runs with
`dotfiles update` but not `dotfiles install`. It depends on
**APM packages**.

Before advancing versions, it verifies that the installed state corresponds to
the current merged-manifest fingerprint. If install convergence did not succeed,
or the desired state changed afterward, update is skipped rather than mutating
an unrelated or partial lockfile.

An update preview runs non-mutating `apm outdated -g` and emits `~` only
when APM reports outdated dependencies. Apply mode invokes APM's native
idempotent update directly, then compares the lockfile before and after to
report whether refs advanced instead of parsing update output. The comparison
ignores APM's volatile `generated_at` bookkeeping stamp, which is rewritten on
every serialization — including the follow-up `copilot-app` redeploy — so only
real dependency-state changes are reported.

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

`dotfiles test` includes **Validate APM plugins**. When APM is available, the
check validates active plugin and package references. If APM is not installed,
the check is reported as unavailable rather than silently treated as executed.

APM changes should also preserve:

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

Do not manually edit generated merged state or lock data when the same change
can be represented in a source fragment.
