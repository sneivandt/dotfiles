---
title: Agent deployment internals
description: Maintain APM fragment merging, deployment ownership, Copilot App workflow repair, and Cowork skill copying.
---

# Agent deployment internals

This guide is for contributors changing the agent deployment implementation.
To add a skill, package, or preference, start with
[AI agent configuration](ai-agent-configuration.md). That guide owns the
user-facing setup, target selection, and removal instructions.

The implementation lives under
[`cli/src/domains/ai/apm/`](https://github.com/sneivandt/dotfiles/tree/main/cli/src/domains/ai/apm/).
Native APM owns dependency resolution, local-source verification, and stale
deployment cleanup. Dotfiles owns fragment merging and two compatibility
integrations: Copilot App workflow policy and Cowork skill copying.

## Update sequence and ownership

An applying `agent-packages` update:

1. Reads the selected source fragments and any additional home fragments.
2. Atomically updates the generated `~/.apm/apm.yml` if needed. A symlink at
   that path is rejected rather than followed.
3. Runs `apm update -g --yes`, then `apm install -g`.
4. Applies the detected Copilot App and Cowork integrations.
5. Reports manifest, lockfile, and integration changes.

The follow-up install is deliberate. An update can find no newer dependency
revisions while local plugin edits or missing/changed deployed files still
need applying. Run the install pass even when the refs and manifest are
unchanged, and before both target adapters.

Keep native APM responsible for deciding what needs deployment. Separate
dotfiles prune commands, success fingerprints/markers, or decisions based on
`apm outdated` would create a second, potentially conflicting policy.
Preserve authentication-error classification and workflow-policy restoration
when any phase fails.

See [`install.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/apm/install.rs),
[`commands.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/apm/commands.rs),
and [`managed_targets.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/apm/managed_targets.rs).

### Preview boundary

Pin-update previews require APM on PATH. If the generated manifest already
matches the fragments, preview invokes `apm update -g --dry-run` and summarizes
its plan. Otherwise it describes the manifest write and update/install
commands rather than querying APM with stale inputs.

Neither preview path runs the follow-up install, target repair, legacy lock
cleanup, or Cowork inventory writes. Native preview may contact upstream
services. Wrapper bootstrap, CLI bookkeeping, and external-tool effects remain
outside this non-mutation guarantee; see
[Dry-run scope](commands-and-usage.md#dry-run-scope).

## Fragment merge contract

Effective inputs include selected source files, even before their home links
exist. A selected source takes the place of a home fragment with the same
filename; other YAML files under `~/.apm/config/` still participate.
This allows a preview to use current source even when a managed link is stale
or broken.

Merge order is by effective filename, not by public/private origin. Both
`.yml` and `.yaml` are supported. Each nonempty fragment must be a mapping.

| Input | Result |
|---|---|
| `name`, `version` | Always generated as `dotfiles`, `1.0.0` |
| `dependencies`, `devDependencies` | Groups append in filename order |
| Named MCP entries within a dependency group | First entry for a name wins |
| Other dependency entries, including unnamed MCP entries | Identical serialized entries deduplicate; first wins |
| Other mappings | Merge recursively |
| Other sequences, including `targets` | Append unique values |
| Other scalar values or differing value types | Later value replaces earlier value |

The distinction between named MCP entries and other dependencies matters:
an MCP server name is its merge identity, while other entries deduplicate by
serialized content. This is not the TOML overlay merge.
Exact examples and regressions live in
[`fragments.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/apm/fragments.rs).

## Target adapters

Native target precedence is invocation `--target`, manifest `targets`,
configured APM default, then detection. The primary dotfiles invocation leaves
the manifest in control. The adapters below are additional host-detected
integrations, not ordinary entries in that target list.

### Copilot App workflow repair

When `~/.copilot/data.db` exists, dotfiles enables experimental `copilot-app`
support when needed, then invokes:

```text
apm install -g --target copilot-app --only apm
```

The `--only apm` filter deploys workflow packages without forwarding
manifest-wide MCP dependencies to a target that cannot accept them.

After deployment, repair restores managed workflows to enabled, autopilot
state and repairs next-run schedules. Its ownership rules are deliberately
narrow:

- Only exact workflow IDs in the global APM lockfile are managed. An `apm--`
  prefix or visible definition alone does not establish ownership.
- Duplicate rows collapse within a managed ID. A legacy
  `apm--unknown--<package>--<prompt>` row is removed only when its corresponding
  managed `apm--_local--<package>--<prompt>` and definition match.
- For local dependencies, prompt sources identified by `local_path` restore
  `cron_expression` omitted by native deployment. Changing/removing source cron
  metadata changes/removes the cron schedule. The App stores cron as
  `interval: manual` plus `cron_expression`.
- Missing or invalid sources for live managed workflows produce a warning,
  not an invented manual schedule. Stale lock entries for workflows absent
  from the database do not block the remaining repairs.
- Remote dependencies retain native scheduling; existing cron schedules
  without a local source survive. Future local schedules use the offset at the
  scheduled date, including daylight-saving changes.

Repair needs Python and database access. It is best-effort, with warnings
reported separately from the native result. Attempt it even after an APM or
Cowork failure: an earlier step may already have reset workflow policy.
Implementation and regression tests live in
[`autopilot/`](https://github.com/sneivandt/dotfiles/tree/main/cli/src/domains/ai/apm/autopilot/).

### Cowork copy and cleanup

The [Cowork destination rules](ai-agent-configuration.md#cowork-skills)
determine whether this integration participates. Dotfiles enables the
experimental feature but does **not** invoke native `copilot-cowork`
deployment. It copies resolved skills from `~/.agents/skills` file-by-file,
preserving Cowork/OneDrive directories and their Windows access-control lists.
Lockfile `target_subset` filters apply: only unfiltered packages or those
including `copilot-cowork` contribute skills.

The copier preserves matching links, replaces changed file/link entries
without writing through destination links, and refuses linked directory
destinations or directory-to-file replacements. Extra Cowork files remain.

Successful skills are recorded in `.dotfiles-apm-skills.json` at the
destination. For a removed or excluded skill, cleanup removes only its
`SKILL.md`, and only with inventory-proven ownership. Directories, placeholders,
other files, and unrelated skills survive. Windows case-only renames preserve
the desired entry point.

Before native deployment, legacy `cowork://` lock records are removed to prevent
native directory cleanup. Explicit legacy skill ownership is retained in the
inventory when the destination exists. The inventory is required for safe
cleanup; deleting it is not a repair strategy.
See [`cowork.rs`](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/apm/cowork.rs).

## Validating implementation changes

Dotfiles checks that local `~/.apm/plugins/dot-*` references have source
directories in the repository or overlay that declares them. Native
`apm pack --dry-run --verbose`, not a second Rust schema, validates package
shape. Packing alone does not prove deployment compatibility.

Use isolated homes, App databases, and controlled executors for deployment
regressions. Do not deploy to the live home to test an implementation change.
Choose commands from [Testing](testing.md#choosing-coverage).

Neither adapter should be removed merely because APM's version changed.
A replacement needs evidence for target filtering, ACL-safe updates, owned
stale cleanup, workflow policy, and preservation of foreign workflows on the
relevant platforms. Include failure-after-partial-deployment cases so policy
restoration remains covered.
