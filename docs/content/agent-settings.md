---
title: Agent settings
description: Copilot and Codex preferences.
---

# Agent settings

| Task | Selection |
|---|---|
| Selector | `agent-settings` |
| Commands | `update` |

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

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only agent-settings --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

Stored preferences remain after a declaration is removed. This task has no remove
action. Restore the values you want in the application settings or deliberately remove
the keys from its settings file.
