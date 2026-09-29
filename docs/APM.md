# APM and AI tooling

[APM](https://github.com/microsoft/apm) resolves and deploys reusable agent
content. Dotfiles selects its inputs, generates a user-scope manifest, and
invokes APM. Harness preferences such as reasoning effort are managed separately.

Start with the ownership table before editing anything. In particular, do not
hand-edit the generated manifest or deployed copies of a skill.

## Responsibilities

| What you want to change | Edit here | What happens on apply |
|---|---|---|
| Copilot or Codex preferences | [`conf/agent-settings.toml`](../conf/agent-settings.toml) | Selected keys converge in `~/.copilot/settings.json` or `~/.codex/config.toml` |
| Package dependencies, MCP declarations, or normal deployment targets | [`symlinks/apm/config/`](../symlinks/apm/config/) | Active fragments merge into `~/.apm/apm.yml`; APM resolves and deploys them |
| Which machines receive a fragment or local plugin | [`conf/symlinks.toml`](../conf/symlinks.toml) | Profile/category selection exposes the source under `~/.apm/` |
| Repository-owned reusable agent content | [`symlinks/apm/plugins/`](../symlinks/apm/plugins/) | Linked local plugins are consumed by APM |
| Instructions for working on this repository | [`AGENTS.md`](../AGENTS.md) and [`.agents/skills/`](../.agents/skills/) | Repository-local guidance; not the personal plugin deployment tree |

`dot-agent` holds reusable agent behavior; `dot-skill` holds focused reusable
skills. Private integrations and instructions belong in a private overlay.
Do not duplicate APM-managed content directly into harness directories.

Only declared harness keys are managed; invalid existing settings documents
are reported without replacing them. Removing a declaration leaves its stored
value in place. See [agent settings](CONFIGURATION.md#agent-harness-settings)
for the schema. Codex's user config is shared by its CLI, IDE extension, and
desktop agent; app-only appearance and notification preferences belong to the
app, and ChatGPT Work chats do not read local Codex settings.

**Applying APM is executable configuration, not just copying text.** Packages
can supply hooks, MCP servers, and workflows. When Copilot App is present,
dotfiles also enables its own deployed workflows in autopilot mode. Review
those sources and [target-specific behavior](#deployment-targets) before applying.

## Configuration fragments

Tracked fragments live in `symlinks/apm/config/`. The symlink configuration
selects which ones become `~/.apm/config/*.yml` or `*.yaml`. A fragment for one
platform must be selected by the matching category; a filename such as
`arch.yml` is not itself a condition.

During planning, dotfiles reads the selected **source** files, even if their
home links have not been installed yet. A managed source masks the home entry
with the same filename before inspecting that entry. This lets dry-run plan
the replacement of a stale or broken managed link. Other, unmanaged YAML
fragments already in `~/.apm/config/` also participate.

Fragments are ordered by their effective filename in that directory, not by
"public first, private last." For example, `base.yml` sorts before `private.yml`,
but `90-private.yml` sorts before `base.yml`. Choose names deliberately rather
than relying on overlay precedence.

### Merge rules

Each nonempty fragment must be a YAML mapping. Dotfiles merges it as follows:

| Field | Rule |
|---|---|
| `name`, `version` | Generated identity is fixed to `dotfiles`, `1.0.0` |
| `dependencies`, `devDependencies` | Dependency groups append in fragment order |
| MCP dependency entries | First entry with a given `name` wins; unnamed entries deduplicate by serialized value |
| Other dependency entries | Duplicate serialized entries are removed, keeping the first |
| Other mappings | Merge recursively |
| Other sequences, including `targets` | Append unique values |
| Other scalar values | Later values replace earlier values |

These rules differ from [TOML overlay merging](CONFIGURATION.md#merge-rules).
An overlay can add targets, but omitting a base target from its own list does
not remove it. Similarly, repeating an MCP server's name is not an override.
Keep one authoritative declaration when changing an existing server.

The merge implementation and its examples live in
[`fragments.rs`](../cli/src/domains/ai/apm/fragments.rs).

## Adding an APM package

1. Edit the narrowest applicable source fragment. Use the native APM dependency
   syntax and a deliberate ref/pin policy rather than editing lock data.
2. For a local package, put it under `symlinks/apm/plugins/` and ensure the
   declaring repository selects it through `conf/symlinks.toml`.
3. If introducing a fragment, select its link in the same categories as its
   required local plugins. Keep target compatibility in the package declaration.
4. Follow [validation](#validation), then preview against the edited checkout.
5. Apply ordinary install to reconcile declarations. Use update only when
   advancing dependency refs is intended.

The base fragment illustrates the local-package form:

```yaml
targets:
  - agent-skills
  - copilot

dependencies:
  apm:
    - ~/.apm/plugins/dot-agent
    - ~/.apm/plugins/dot-skill
```

This is a fragment example, not a second file to add alongside the existing
base fragment.

## Install behavior

The `apm` task runs after regular packages, AUR packages, and symlinks so its
executable and managed sources can be available. A scoped invocation still
needs those prerequisites already satisfied; see [task selection](TASKS.md).

On an ordinary apply, dotfiles:

1. Discovers effective fragments and produces a deterministic manifest.
2. Writes `~/.apm/apm.yml` atomically only when its contents differ. An existing
   symlink at that generated path is rejected, not followed.
3. Runs `apm install -g` without a primary `--target` override.
4. Reconciles available Copilot App and Cowork targets as described below.
5. Compares the exact before/after `~/.apm/apm.lock.yaml` and adapter state to
   report changes.

Native APM owns package resolution, local-source verification, and stale
deployment cleanup. Install does not intentionally advance pinned dependencies.
It still invokes APM when the manifest is unchanged, so deployment drift can be
repaired.

Run from the repository root with an already available CLI:

```bash
dotfiles install --root . --no-repo-update --only apm --dry-run --verbose
```

After reviewing the plan, the applying command is:

```bash
dotfiles install --root . --no-repo-update --only apm
```

Ordinary install dry-run describes the manifest, native command, and target
work without invoking the native install or writing those artifacts. It can
preview installation even before `apm` is present. This does not make wrapper
bootstrap or all CLI bookkeeping read-only; see [Usage](USAGE.md).

## Pin-update behavior

`dotfiles update` (or `install --update`) changes the native operation to
`apm update -g --yes`. APM advances eligible refs and converges the resulting
dependency graph in that pass.

Preview the scoped update:

```bash
dotfiles update --root . --no-repo-update --only apm --dry-run --verbose
```

If the generated manifest is already current, dotfiles invokes the native
`apm update -g --dry-run` and promotes dependency changes from its plan into
the task output. This may contact upstream services and requires APM.
If fragments would change the manifest, it reports the prospective manifest
write and update command instead of asking APM to plan against stale inputs.

Remove `--dry-run` only to apply the update. Both modes use lockfile changes,
not installation chatter, to identify changed dependencies. Output can name
ref, commit, content, deployment-file, or target changes. Cowork repairs and
workflow-policy repairs count as changes even when the lockfile is identical.
Native details remain in the run log and verbose output.

## Deployment targets

The base fragment explicitly selects `agent-skills` and `copilot`, avoiding
deployment to every harness directory APM happens to discover. Native APM
target resolution is:

1. Invocation `--target`.
2. Merged manifest `targets`.
3. APM's configured default target.
4. Harness auto-detection.

The primary dotfiles invocation leaves the manifest in control. The two
compatibility paths below are **additional, host-detected behavior**, not
ordinary entries in that target list.

### Copilot App workflows

When `~/.copilot/data.db` exists, dotfiles ensures APM's experimental
`copilot-app` support is enabled and runs a separate:

```text
apm install -g --target copilot-app --only apm
```

The separate invocation deploys workflow packages without sending manifest-wide
MCP dependencies to an MCP-incapable target. It runs in update mode too, after
the primary update resolves dependencies.

Dotfiles then restores its managed workflows to **enabled, autopilot** state
and repairs `next_run_at`. This is an intentional policy: manually disabling
one of those workflows in the App is not a persistent opt-out from future
dotfiles convergence.

Ownership comes from the exact workflow IDs recorded in the global APM
lockfile, not a broad name prefix. Duplicate rows are collapsed only within a
managed ID. A legacy `apm--unknown--<package>--<prompt>` row is removed only when
the corresponding managed `apm--_local--<package>--<prompt>` and its definition
match. Foreign workflows and independent IDs remain untouched even when their
visible definitions match.

Custom cron expressions are retained, including the App's representation as
`interval: manual` plus `cron_expression`. Future local schedules use the UTC
offset at the scheduled date, including daylight-saving transitions.

Restoration is attempted even after an APM or Cowork failure, since an earlier
step may already have reset workflows. It is best-effort: failures are reported
separately and do not replace the original convergence result. Python and access
to the App database are required for this repair. See
[`autopilot/`](../cli/src/domains/ai/apm/autopilot/) for the ownership and
failure-handling contracts.

### Cowork skills

Cowork reconciliation is enabled when a skills path resolves, in this order:

1. `APM_COPILOT_COWORK_SKILLS_DIR`.
2. `copilot_cowork_skills_dir` in `~/.apm/config.json`.
3. On Windows, `ONEDRIVECOMMERCIAL`, then `ONEDRIVE`, with
   `Documents/Cowork/skills` appended.

An explicit path also works on Linux. Dotfiles reasserts the experimental
feature when needed, but deliberately does **not** invoke APM's native
`copilot-cowork` deployment. It copies resolved skills from `~/.agents/skills`
file-by-file to avoid replacing OneDrive-protected directories and their ACLs.
Reconciliation retains unchanged symlinks, repairs changed file/link entries
without writing through destination links, and refuses to replace directories
with files.
Lockfile `target_subset` filters are respected: only unfiltered packages or
packages including `copilot-cowork` are candidates.

Successful copies are recorded in `.dotfiles-apm-skills.json` in the Cowork
skills directory. For excluded or removed skills, reconciliation removes
`SKILL.md` only when that inventory establishes ownership. It preserves
directories, Cowork placeholders, and unrelated skills. A same-named source
under `~/.agents/skills` alone is not proof that an older copy is managed.

Before native convergence, dotfiles removes legacy `cowork://` lockfile
deployment records that could provoke directory deletion. Explicit legacy skill
records are first retained in the ownership inventory when the configured
directory exists. Dry-run modifies neither that inventory, the lockfile, nor
Cowork content.

These adapters address known upstream behavior, including the directory
replacement in APM's
[v0.29.1 skill integrator](https://github.com/microsoft/apm/blob/v0.29.1/src/apm_cli/integration/skill_integrator.py)
and workflow defaults in its
[workflow integrator](https://github.com/microsoft/apm/blob/v0.29.1/src/apm_cli/integration/copilot_app_workflow_integrator.py).
Do not remove them merely because APM has a newer version. A replacement must
demonstrate ACL-preserving updates, stale cleanup, target filtering, workflow
policy retention, and preservation of foreign workflows on the relevant
platforms.

## Removing packages and targets

Remove a package from its source fragment, then run ordinary APM convergence
while a fragment still exists. Native APM handles stale deployments; the Cowork
adapter uses its ownership inventory for its narrower cleanup.

To remove a normal deployment target, remove it from **every** contributing
fragment: target lists merge by union. Host-detected Copilot App and Cowork
behavior is separate from this list.

If no managed or unmanaged fragments remain, the task is inapplicable. Removing
the last fragment does **not** invoke APM cleanup or delete the existing
manifest, lockfile, or deployments. Keep a valid fragment while converging the
intended empty dependency set; do not assume deleting source files uninstalls
their deployed content.

## Overlays

An overlay can contribute fragments and local plugins using the same layout.
Keep private source locations and content in that overlay. Select links with
compatible categories, use distinct home targets, and validate the combined
configuration rather than each repository in isolation.

An unmanaged home fragment is another effective input. If the merged manifest
contains an unexpected dependency, inspect all fragment **sources and names**
before editing generated state.

## Validation

Dotfiles validates its cross-file invariant that a local
`~/.apm/plugins/dot-*` reference has a matching source directory in the declaring
repository or overlay. Fragment merging checks the YAML shapes it consumes;
native APM owns the full dependency, MCP, and package-layout schema.

The `check` command's APM plugin validator runs `apm pack --dry-run --verbose`
for local plugin directories. APM being unavailable is not evidence that a
plugin passed validation. Use [Testing](TESTING.md#cli-validation) for the exact
commands and [focused coverage](TESTING.md#choosing-coverage) when changing
implementation behavior.

For a content change, check package layout, source availability, fragment/link
category alignment, target compatibility, and the scoped dry-run. A successful
pack alone does not prove deployment works in every target.

## Authentication and failure diagnosis

APM subprocesses disable interactive Git credential prompts. Recognized
authentication failures are reported as unmet prerequisites, not a successful
deployment; other native failures fail the task. Authenticate deliberately
outside the run, for example with `gh auth login`, and retry. Never put tokens
in fragments, plugin content, or diagnostic excerpts.

For a failure, use the task's concrete error and saved verbose output to
distinguish fragment parsing, credentials, missing tools, native deployment,
and adapter repair. Do not clear lockfiles or whole deployment trees as a
first response. A failed multi-step run may already have changed the manifest
or some targets; correcting the cause and reconverging is safer than treating
it as an automatic rollback.
