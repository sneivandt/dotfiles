---
title: Agent packages
description: Install and update APM agent packages, merge dependency fragments, and deploy reusable skills and plugins to supported harnesses.
---

# Agent packages

[Agent Package Manager](https://github.com/microsoft/apm) (APM) resolves and
deploys reusable agent content such as skills, instructions, and plugins.
`dotfiles` selects and merges your YAML fragments, while APM handles dependency
resolution and deployment. (For application preferences like Copilot or Codex
settings, see [Agent settings](agent-settings.md).)

| Task detail | Value |
|---|---|
| Selector | `agent-packages` |
| Commands | `update` |
| Availability | Active APM fragments |
| [Requires success](task-reference.md#scheduling-model) | [Home symlinks](home-symlinks.md) |
| [Runs after](task-reference.md#scheduling-model) | [System packages](system-packages.md), [AUR packages](aur-packages.md), conditional on readiness |
| Implementation | [ai/apm/install.rs](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/apm/install.rs) |

## What it does

Merges active main and overlay APM fragments, updates dependencies within their
declared version bounds, and deploys agent content for the current user. It
requires `symlinks` to succeed and waits for package/AUR tasks unless a
readiness probe confirms `apm` is already available.

See [Agent configuration](../guides/agent-configuration.md) for targets,
authentication, cleanup, and
[what a preview can show](../guides/agent-configuration.md#what-a-preview-can-show).

## Configuration

Agent packages are declared in YAML fragments under
[symlinks/apm/config/](https://github.com/sneivandt/dotfiles/tree/main/symlinks/apm/config/),
enabled through `conf/symlinks.toml` (separately from `agent-settings.toml`).
For example, the base fragment lists allowed targets and local plugins:

```yaml title="symlinks/apm/config/base.yml (excerpt)"
targets:
  - agent-skills
  - copilot
dependencies:
  apm:
    - ~/.apm/plugins/dot-skill
    - ~/.apm/plugins/dot-workflow
```

Enable a fragment by linking it in `conf/symlinks.toml`, keeping the fragment
and any local plugins it references in matching categories. Always edit the
tracked fragments rather than the generated merged manifest, which the next run
can overwrite.

See the [APM reference](https://microsoft.github.io/apm/reference/) for native
APM commands, and [Agent configuration](../guides/agent-configuration.md) for
user-scope targets and trust rules (remember that a package can introduce
executable hooks or tools, not just Markdown).

## Preview and apply

Preview deployment from the active APM fragments:

```sh
dotfiles update --only agent-packages --dry-run
```

Check the target harnesses and reported dependency additions, updates, or
removals (keeping in mind
[what a preview can show](../guides/agent-configuration.md#what-a-preview-can-show)).
Ensure fragment links, `apm`, and any needed auth are ready before
[applying](task-reference.md#preview-and-apply), then verify the deployed files
in your target harnesses.

## Removal

`dotfiles remove` does not touch APM deployments, and removing the last config
fragment can stop APM's own stale-file cleanup from running. Follow
[Removing packages and targets](../guides/agent-configuration.md#removing-packages-and-targets)
while your deployment config is still in place.
