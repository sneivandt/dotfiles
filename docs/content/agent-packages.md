---
title: Agent packages
description: APM YAML fragments selected by symlinks.
---

# Agent packages

| Task | Selection |
|---|---|
| Selector | `agent-packages` |
| Commands | `update` |

## What it does

Updates agent dependencies within their declared version rules and deploys content for
the current user from selected main/overlay APM fragments. It requires successful
symlink work and normally waits for package/AUR tasks. Readiness checks can allow it to
proceed before those package tasks finish.

See [Agent configuration](agent-configuration.md) for sources, targets, authentication,
and cleanup, including [what a preview can
show](agent-configuration.md#what-a-preview-can-show). The [implementation
reference](architecture.md#agent-deployment-implementation) explains the command
sequence and compatibility adapters.

## Configuration

Agent packages use YAML fragments under
[symlinks/apm/config/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/apm/config/),
selected by `conf/symlinks.toml`. They are separate from `agent-settings.toml`
preferences. For example, the base fragment names the allowed destinations and the local
plugins to install:

```yaml title="symlinks/apm/config/base.yml (excerpt)"
targets:
  - agent-skills
  - copilot
dependencies:
  apm:
    - ~/.apm/plugins/dot-agent
    - ~/.apm/plugins/dot-skill
```

Select a fragment by linking it through `conf/symlinks.toml`; use matching categories
for the fragment and any local plugins it requires. These paths point to
plugin links installed in your home directory. See [Agent
configuration](agent-configuration.md) for adding packages, choosing targets, generated
files, and cleanup.

## Preview and apply

With an [installed launcher and saved profile](getting-started.md), preview this task:

```sh
dotfiles update --only agent-packages --dry-run
```

Review the result before repeating the command without `--dry-run`. [Task
selection](commands-and-usage.md#select-tasks) explains when to include dependencies;
[preview scope](commands-and-usage.md#dry-run-scope) describes side effects that a
preview can still have.

## Removal

This task is not part of `dotfiles remove`. APM has its own stale-deployment cleanup,
and removing the last configuration fragment can prevent that cleanup from running.
Follow [Removing packages and
targets](agent-configuration.md#removing-packages-and-targets) while the deployment
configuration is still available.
