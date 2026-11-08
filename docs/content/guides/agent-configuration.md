---
title: Agent configuration
description: How this dotfiles setup separates Copilot and Codex preferences, reusable content deployed through APM, and repository instructions.
---

# Agent configuration

This page explains how I manage my coding-agent setup with this dotfiles
repository: application preferences, reusable skills and integrations, and
instructions for work in a specific repository. It covers the organization
of the setup and the workflow for changing it. The commands assume dotfiles
is installed and a profile is selected; see [Getting started](getting-started.md)
for installation.

The same organization can be useful in another setup. The plugin names below
are examples; the paths, task names, and deployment integrations describe
this repository.

## How the setup fits together

An agent *harness* is the application running the agent, such as Copilot or
Codex. I keep three kinds of configuration separate:

| Part | Purpose | How it reaches the agent |
|---|---|---|
| Application preferences | Model, permissions, interface, and other declared settings | The `agent-settings` task updates selected keys in application settings files |
| Reusable agent content | Skills, instructions, hooks, workflows, and tool integrations used across repositories | The `agent-packages` task uses Microsoft APM to resolve and deploy packages |
| Repository guidance | Project conventions and skills for work in one checkout | The harness reads that repository's `AGENTS.md` and `.agents/skills/` directly, without APM deployment |

A *skill* is a focused set of instructions, optionally with supporting files.
A *plugin* bundles reusable content; local plugins are maintained in this
checkout, while external packages come from dependencies. MCP servers provide
tools the agent can call. APM can manage these as part of package deployment;
it does not manage the Copilot and Codex preferences declared here.

[Microsoft APM](https://github.com/microsoft/apm), the Agent Package Manager,
handles dependency resolution and native deployment. Dotfiles supplies the
surrounding workflow: it selects source files by profile, combines YAML
configuration fragments into one manifest, invokes APM, and reconciles the
additional [Copilot App and Cowork integrations](#detected-integrations).
Installing or upgrading the APM executable is a separate
[package-management](package-management.md) concern.

The sections below start with choosing a source and making a change.
[Configuration fragments](#configuration-fragments) and
[Detected integrations](#detected-integrations) explain the merge and deployment
details when you need them. Private or employer-specific content belongs in a
[private overlay](private-overlays.md); credentials stay outside tracked files.

::: warning Packages can do more than provide text
Review package sources before applying them. Packages can contain executable
hooks, MCP servers (tools the agent can call), and scheduled workflows.
When the Copilot App is detected, dotfiles enables its managed workflows in
**autopilot** mode.
:::

## Choose what to change

A configuration *fragment* is a YAML file containing part of the APM
configuration. Dotfiles combines the selected fragments before deployment.
Choose the source for your change:

| Change | Source |
|---|---|
| Set a Copilot or Codex preference | [conf/agent-settings.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/agent-settings.toml) |
| Add an external plugin or MCP server | A YAML configuration fragment in [symlinks/apm/config/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/apm/config/) |
| Add or change a local plugin, including reusable skills, instructions, or hooks | A plugin directory under [symlinks/apm/plugins/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/apm/plugins/), with its dependency in a configuration fragment |
| Choose which profiles receive a fragment/plugin | [conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml) |
| Guide contributors to this repository | [AGENTS.md](https://github.com/sneivandt/dotfiles/blob/main/AGENTS.md) or [.agents/skills/](https://github.com/sneivandt/dotfiles/tree/main/.agents/skills/) |

Private overlays use the same fragment/plugin layout and compatible categories.
Never edit the generated combined configuration, `~/.apm/apm.yml`, lock data,
or deployed harness copies as a substitute for changing their sources.

## Agent preferences

The `agent-settings` task edits only declared keys in these documents:

| Application | User settings |
|---|---|
| `copilot` | `~/.copilot/settings.json` |
| `codex` | `~/.codex/config.toml` |

Unmanaged keys and TOML comments survive. If a requested key is invalid or its
parent has an incompatible value, that document is left unchanged. Detected
concurrent edits stop the write; resolve the conflict before retrying.
Applications that do not coordinate with dotfiles can still write at the same time.
Invalid existing documents are reported, not replaced.
Removing a declaration stops managing the key; it does **not** delete the stored
value. Conflicting declarations for the same harness/key fail before writes.
The [Agent settings task](../reference/agent-settings.md) owns the declaration format and
update behavior. With dotfiles installed and a profile selected, edit
`conf/agent-settings.toml` and preview preference changes separately:

```sh
dotfiles update --only agent-settings --dry-run
```

Review the changed keys, then remove `--dry-run` to apply.

See [Agent settings](../reference/agent-settings.md#configuration) for the
schema. Use the source file for current preferences, including declared
Codex desktop settings; do not assume all settings apply to every agent surface.

## Add a skill or package

First choose the audience: repository-only guidance belongs in `.agents/skills/`,
reusable public content in a local plugin, and private content in an overlay.

### Add a skill to a local plugin

Use `example-tools` as a fictional plugin name in this walkthrough. Replace it
with a name that describes your reusable content; a plugin can contain one
skill or several related kinds of content.

If the plugin already exists, add
`symlinks/apm/plugins/example-tools/.apm/skills/change-summary/SKILL.md`:

```markdown title="symlinks/apm/plugins/example-tools/.apm/skills/change-summary/SKILL.md"
---
name: change-summary
description: Use when asked to summarize a code change for a reviewer.
---

Explain the user-visible change first, then the reason for it.
Separate remaining limitations from completed work.
```

For a new plugin, also create `symlinks/apm/plugins/example-tools/apm.yml`.
Use an [existing local package manifest](https://github.com/sneivandt/dotfiles/blob/main/symlinks/apm/plugins/dot-skill/apm.yml)
as a template, changing its name and description for your content. Keep
`includes: auto` to discover skills and other content beneath `.apm/`.
Then add the plugin dependency as described below. Adding a skill to an
already-selected plugin needs no new dependency or symlink declaration.

Validate the package and preview deployment before applying it, as described
[below](#validate-preview-and-apply). Packing checks the package's structure;
it does not prove the instructions are useful or compatible with every target.

### Add a package or configuration fragment

The combined APM configuration is called a *manifest*. This illustrative
fragment selects the example plugin and two deployment targets:

```yaml title="Example APM configuration fragment"
targets:
  - agent-skills
  - copilot

dependencies:
  apm:
    - ~/.apm/plugins/example-tools
```

For the walkthrough, add the dependency under `dependencies.apm` in the existing
[base.yml](https://github.com/sneivandt/dotfiles/blob/main/symlinks/apm/config/base.yml);
preserve its other dependencies and explicit targets. This snippet shows the
shape of the configuration, not a replacement for that file.
A *target* names a destination that APM can deploy to;
[Deployment targets](#deployment-targets) explains the two names above.

The current base symlink declaration, `apm/plugins/*` in
[conf/symlinks.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/symlinks.toml),
already covers a new plugin directory here. Its installed link makes the
source available at `~/.apm/plugins/example-tools`. Apply new links before a
scoped deployment, following [Validate, preview, and apply](#validate-preview-and-apply).
A link makes a local plugin available; the manifest dependency selects it for
APM installation.

To add another package:

1. Add its dependency to the narrowest applicable fragment. Choose the
   dependency's version or revision rule deliberately; normal updates can
   fetch newer revisions when that rule allows them.
2. For a new local plugin, provide its `apm.yml` and content below `.apm/`.
   Use forward slashes in local dependency paths on both platforms.
3. If adding a fragment, declare its link in `conf/symlinks.toml`. Keep its
   category selection compatible with the local plugins it requires.
4. Review the destinations below, then follow
   [Validate, preview, and apply](#validate-preview-and-apply).

Keep self-defined MCP servers as direct dependencies unless you intentionally
trust another package to declare them for you. Dependency-level `targets`
restrict a whole package. Split packages if their content needs different
destinations.

## Deployment targets

Keep the base fragment's explicit targets. Without them, APM can discover
application directories you did not intend to manage. The primary dotfiles
deployment uses the targets in the generated manifest.

| Manifest target | Destination |
|---|---|
| `agent-skills` | Shared skills in `~/.agents/skills` |
| `copilot` | MCP configuration in `~/.copilot/mcp-config.json` |

Dotfiles adds two integrations when detected on the host, beyond the primary
manifest-driven APM deployment:

- If `~/.copilot/data.db` exists, APM installs Copilot App workflows and dotfiles
  enables them in autopilot.
- If a Cowork path is configured or detected, dotfiles copies selected shared
  skills there.

These are not ordinary entries in the base target list. Removing a manifest
target does not disable them. See [Detected integrations](#detected-integrations)
for their selection and cleanup rules.

## Validate, preview, and apply

On a machine with dotfiles, a selected profile, and APM on PATH:

```sh
dotfiles check --only config-diagnostics,symlink-sources,apm-plugins
dotfiles update --only agent-packages --dry-run
```

The checks verify source/link configuration and run
`apm pack --dry-run --verbose` in local plugin directories. If APM is missing,
the APM check is skipped. Private plugins need validation in their own
source context too.

A scoped agent run does not install prerequisites. If you added plugin links,
apply the symlink task first; for a first setup, use the normal
[update walkthrough](getting-started.md). A normal update coordinates agent
deployment with symlink and package installation. APM can come from any
installation method as long as `apm` is on PATH.

### What a preview can show

A dry run requires APM on PATH. What it can show depends on whether the
generated manifest matches your source fragments:

| Generated manifest | Preview behavior |
|---|---|
| Already matches the fragments | Runs `apm update -g --dry-run` and summarizes the native plan |
| Would change | Describes the manifest write and update/install commands instead of asking APM to inspect stale inputs |

Neither path deploys content, writes the generated manifest, or changes the
Copilot App or Cowork integrations. APM's preview may contact upstream services.
The general [dry-run limits](commands-and-usage.md#dry-run-scope) still apply.

### Apply the reviewed changes

Review the sources, dependency changes, and destinations. Applying combines
fragments into `~/.apm/apm.yml`, updates dependencies within their declared version
rules, and installs content, including local edits with unchanged dependency refs.
It then applies detected integrations. When you intend to deploy to your home:

```sh
dotfiles update --only agent-packages
```

This is a real deployment, not a way to test a skill edit in isolation.
It can run package code and enable scheduled workflows. For preference changes,
follow [Agent preferences](#agent-preferences). On Arch, the APM executable comes
from the [apm-dotfiles package](package-management.md#custom-arch-packages);
upgrading that application is separate from deploying agent content.

## Configuration fragments

The symlink configuration selects source fragments that appear under
`~/.apm/config/`. Names ending in `.yml` or `.yaml` are supported. A filename
such as `arch.yml` does not select a platform: its symlink category does.

```mermaid
flowchart TB
    accTitle: From configuration fragments to agent deployment
    accDescr: Selected managed sources and unmanaged home fragments merge by filename into a generated apm.yml manifest, which APM uses for deployment.
    Sources["Selected source fragments"] --> Merge["Merge by filename"]
    Home["Other home fragments"] --> Merge
    Merge --> Manifest["Generated apm.yml"]
    Manifest --> APM["APM deployment"]
```

Dotfiles reads the selected source fragments directly, so previews use your
edits even if a fragment's home link is missing or broken. Other YAML files
already in `~/.apm/config/` also contribute unless a selected source has the
same filename.

Fragments merge in filename order. A private fragment has no special priority:
`private.yml` sorts after `base.yml`, but `90-private.yml` sorts before it.
This differs from the
[TOML overlay merge](private-overlays.md#merge-rules).

### Fragment merge rules

Each nonempty fragment must contain a YAML mapping. An empty list does not clear
entries from another fragment. Merge behavior is:

| Input | Result |
|---|---|
| `name`, `version` | Always generated as `dotfiles`, `1.0.0` |
| `dependencies`, `devDependencies` | Groups append in filename order |
| Named MCP entries within a dependency group | First entry for a name wins |
| Other dependency entries, including unnamed MCP entries | Identical serialized entries deduplicate; first wins |
| Other mappings | Merge recursively |
| Other sequences, including `targets` | Append unique values |
| Other scalar values or differing value types | Later value replaces earlier value |

Keep each named MCP server in one fragment; later declarations cannot override
it. Exact merge examples and regressions live in
[fragments.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/apm/fragments.rs).

## Detected integrations

These are additional dotfiles integrations around APM. They depend on the
host, not just the manifest's target list; the base deployment does not imply
that every harness receives every kind of content.

### Copilot App workflows

When the App database exists, dotfiles enables APM's experimental `copilot-app`
support and installs workflow packages after the primary deployment. It does
not send manifest-wide MCP dependencies to this workflow-only integration.

::: warning Disabling a managed workflow in the App is temporary
Dotfiles restores its managed workflows to **enabled, autopilot** state and
repairs their next-run schedules. The next agent-package update can enable a
workflow you disabled in the App. Change the package source or dependency
selection for a lasting change.
:::

Workflow repair needs Python and database access. A failed repair fails the task
even when APM succeeded. Missing prerequisites are reported as unmet work and
fail strict runs (`--fail-on-skip` or CI). If deployment itself fails, dotfiles
still attempts repair without replacing the original failure.

### Cowork skills

Cowork is experimental and receives **skills only**. Those skills must remain
useful without MCP servers, hooks, prompts, agents, commands, or instructions.
The destination is the first available value in this order:

1. Nonempty `APM_COPILOT_COWORK_SKILLS_DIR`.
2. `copilot_cowork_skills_dir` in `~/.apm/config.json`.
3. On Windows, `ONEDRIVECOMMERCIAL`, then `ONEDRIVE`, with
   `Documents/Cowork/skills` appended.

Set `APM_COPILOT_COWORK_SKILLS_DIR` to choose a destination explicitly,
including on Linux. Dotfiles copies selected skills from `~/.agents/skills`
while preserving the destination directories and their Windows permissions.
If a package restricts its targets, include `copilot-cowork` to send its skills
here.

Dotfiles records deployed file hashes and removes unchanged owned files that
disappear from the source or leave the selected skill set. Dotfiles can take
ownership of an existing destination file if it matches the source. It reports a conflict when
a destination file differs from both the source and its last deployed version,
or when a stale file has local edits. Move the conflicting file aside before
retrying. Unrelated skills and unowned files remain. Keep the destination's
`.dotfiles-apm-skills.json` and `.dotfiles-apm-files.json` ownership records.
For older deployments, Dotfiles removes a stale `SKILL.md` only when its
recorded hash matches. Move unverified files aside to continue cleanup. For
path-resolution failures or copy conflicts, check the selected destination
above and the reported ownership conflict before retrying.

## Removing packages and targets

Remove a dependency from its source fragment, then run the agent packages
task while a valid fragment remains. Native APM handles stale deployments;
Cowork uses the narrower inventory-based cleanup described above.

To remove a normal target, remove it from every contributing fragment because
target lists combine. The detected Copilot App and Cowork integrations are
controlled separately.

**Deleting the last fragment is not an uninstall.** With no managed or unmanaged
fragments, the task does not run. It leaves the generated manifest, lockfile,
and deployments alone. Keep a valid fragment while applying the intended empty
dependency set.

To remove all declared dependencies while retaining the base targets, keep a
selected fragment with:

```yaml
targets:
  - agent-skills
  - copilot

dependencies:
  apm: []
```

Remove dependencies from every other selected source and unmanaged home fragment
too, including `mcp`, `lsp`, and `devDependencies` groups. Lists combine, so an
empty list cannot clear another fragment's entries. Keep the retained fragment's
symlink declaration active.

Preview with `dotfiles update --only agent-packages --dry-run`,
then remove `--dry-run` after reviewing the source changes and destinations.
When the manifest needs changing, preview describes delegated commands rather
than enumerating removals. Check the run log and affected targets before deleting
the retained fragment. See [preview behavior](#what-a-preview-can-show).

## Troubleshoot deployment

| Symptom | Next step |
|---|---|
| `apm` unavailable | Install it deliberately for the platform; a scoped agent task cannot bootstrap its own prerequisites |
| Authentication prerequisite unmet | Authenticate outside the run, for example with `gh auth login`, then retry |
| Unexpected dependency or target | Inspect effective fragment names and merge rules |
| Workflow repair warning | Check Python, App database access, and the managed prompt sources |
| Cowork path/copy failure | Check destination precedence, directory/link conflicts, and the ownership inventory |
| Package edits did not deploy | Confirm source links and inspect verbose native install output |

Use `dotfiles log --id <run-id> --task agent-packages --verbose`, replacing
`<run-id>` with the stable ID from the failed run. Omitting the ID deliberately
selects the newest run, which may no longer be the failure you are investigating.

APM subprocesses cannot open interactive Git credential prompts. Recognized
authentication failures are reported as unmet prerequisites; other native
failures fail the task. Keep tokens out of fragments and diagnostic excerpts.

A failed run can already have changed the manifest or some targets. There is
no automatic rollback. Correct the reported cause and rerun; deleting lockfiles,
whole deployment trees, or App data is not a safe first response.
