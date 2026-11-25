---
title: Agent settings
description: Manage Copilot JSON and Codex TOML preferences while preserving undeclared settings and applying private overlay overrides.
---

# Agent settings

This task updates selected preferences inside coding-agent harness config files
(Copilot and Codex) without touching undeclared keys, plugin dependencies,
accounts, or skills. See
[How the setup fits together](../guides/agent-configuration.md#how-the-setup-fits-together)
for how harness preferences, APM packages, and repo instructions relate.

| Task detail | Value |
|---|---|
| Selector | `agent-settings` |
| Commands | `update` |
| Availability | Configured harness settings |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [ai/agent_settings.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/agent_settings.rs) |

## What it does

Writes declared dot-separated keys into Copilot JSON and Codex TOML config
files while preserving undeclared and harness-owned settings. It never deploys
skills or plugins.

## Configuration

Declare settings in
[conf/agent-settings.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/agent-settings.toml)
under `[<category>.<target>]` sections:

```toml
[base.copilot]
footer.showBranch = true

[base.codex]
model_reasoning_effort = "medium"
```

| Target | Document |
|---|---|
| `copilot` | `~/.copilot/settings.json` |
| `codex` | `$CODEX_HOME/config.toml`, defaulting to `~/.codex/config.toml` |

An unset or empty `CODEX_HOME` falls back to `~/.codex/config.toml`; a nonempty
value must be an absolute path (changing it doesn't migrate or delete the old
file, so run `dotfiles` with the same environment as Codex).

Write keys as dotted paths or nested tables (for example `[base.copilot.footer]`).
Non-empty nested tables flatten to dot-separated leaf keys so undeclared and
harness-owned sibling keys stay untouched; empty tables (`{}`) stay leaf empty
tables. Existing files must parse as valid JSON objects or TOML tables
(malformed files fail rather than being overwritten), updates are atomic, and if
the settings path is a symlink the target file is updated while keeping the link
intact.

Duplicate key names along a managed path inside an existing JSON object stop
preview and apply without rewriting the file; resolve the duplicate first and
retry.

Numbers must be finite (including inside arrays and tables), and conflicting
leaf or parent/child keys fail loading (`footer = false` or `footer = {}`
conflicts with `footer.showBranch = true`). Conflicts within either source are
rejected before overlay merging, and merged parent/child keys are validated too.

An overlay overrides an exact, case-sensitive leaf key for the same harness
while leaving sibling keys intact. Removing a declaration leaves the stored
value in place. Skills, plugins, hooks, and MCP servers belong in
[Agent configuration](../guides/agent-configuration.md), not here.

`dotfiles` checks declaration and file syntax, not whether the harness still
recognizes every key; consult upstream docs such as
[Codex configuration](https://developers.openai.com/codex/config-basic/) when
choosing keys, and check project or policy layers if a stored value appears
overridden.

## Preview and apply

Preview which harness config files would change:

```sh
dotfiles update --only agent-settings --dry-run
```

The preview lists destination files rather than a per-key diff. If an existing
file is malformed, fix it without discarding unrelated keys before
[applying](task-reference.md#preview-and-apply), then inspect the saved file
(and any project/policy overrides if the harness behaves differently).

## Removal

This task has no `remove` action, and deleting a declaration leaves the stored
preference behind. Edit or remove the key directly in the harness's config file
when you no longer want it.
