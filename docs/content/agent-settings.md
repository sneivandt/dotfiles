---
title: Agent settings
description: Copilot and Codex preferences.
---

# Agent settings

An agent harness is the application running the coding agent, such as Copilot
or Codex. This task manages selected preferences in those applications'
settings files. It does not choose plugin dependencies, authenticate an
account, or deploy skills; those are separate concerns.

| Task detail | Value |
|---|---|
| Selector | `agent-settings` |
| Commands | `update` |
| Availability | Configured harness settings |
| [Requires success](task-reference.md#scheduling-model) | None |
| [Runs after](task-reference.md#scheduling-model) | None |
| Implementation | [ai/agent_settings.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/agent_settings.rs) |

## What it does

Applies selected dot-separated keys to Copilot JSON and Codex TOML documents, preserving
undeclared and harness-owned keys. It does not deploy skills or plugins. The declaration
format is below.

## Configuration

Edit
[conf/agent-settings.toml](https://github.com/sneivandt/dotfiles/blob/main/conf/agent-settings.toml).

```toml
[base]
settings = [
  { target = "copilot", key = "footer.showBranch", value = true },
  { target = "codex", key = "model_reasoning_effort", value = "medium" },
]
```

| Target | Document |
|---|---|
| `copilot` | `~/.copilot/settings.json` |
| `codex` | `~/.codex/config.toml` |

`target`, `key` and `value` are required. Keys are dot-separated paths; values use TOML
types. Choose the narrowest key that expresses the preference. Undeclared and
harness-owned keys remain untouched.

Existing documents must be valid JSON objects or TOML tables. Malformed documents fail
rather than being replaced with empty ones. Updates replace the document atomically. If
the settings path is a symlink, the task updates the file it points to and preserves the
link.

Numbers must be finite, including inside arrays or tables. Overlapping keys must agree:
`footer = { showBranch = true }` conflicts with `footer.showBranch = false`, but agrees
with `footer.showBranch = true`. Conflicts within either source are rejected before
overlay replacement; merged parent/child keys are checked too.

An overlay replaces an exact, case-sensitive key for the same harness. Other keys remain
managed. Removing a declaration leaves the stored preference in place. Skills, plugins,
hooks and MCP configuration belong to [Agent configuration](agent-configuration.md), not
this file.

Dotfiles validates the declaration and document shape, not whether the
installed harness still supports every key. Use the harness's current
settings reference when choosing a key, such as
[Codex configuration](https://developers.openai.com/codex/config-basic/).
Project or policy layers may also affect the effective preference. If a
stored value seems ignored, check those layers before broadening the managed
key to replace an entire settings table.

## Preview and apply

Preview which harness documents need changing:

```sh
dotfiles update --only agent-settings --dry-run
```

Check the destination documents and review their declared keys and values;
the preview summarizes documents, not a per-key diff. If an existing document
is malformed, repair it without discarding unrelated settings before
[applying the change](task-reference.md#preview-and-apply). Inspect the stored
keys afterward, then check the harness's effective settings if project or
policy layers override them.

## Removal

Stored preferences remain after a declaration is removed. This task has no remove
action. Restore the values you want in the application settings or deliberately remove
the keys from its settings file.
