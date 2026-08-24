---
name: ai-tooling-apm
description: >
  Use when editing this repo's conf/agent-settings.toml, symlinks/apm/ files,
  or cli/src/domains/ai/apm implementation. Not for general skill-writing
  advice, audience/placement decisions, or unrelated agent behavior.
---

# AI Tooling and APM

## Choose the owner

| Change | Location |
|---|---|
| repo-specific coding skill | `.agents/skills/` |
| reusable personal skill, hook, prompt, or instruction | `symlinks/apm/plugins/dot-*` |
| external plugin or MCP dependency | `symlinks/apm/config/*.yml` |
| stable Copilot/Codex preference | `conf/agent-settings.toml` |
| install/update behavior | `cli/src/domains/ai/apm.rs` and `cli/src/domains/ai/apm/` |

Use the global `apm-dotfiles` skill for public-vs-private placement and plugin
audience decisions.

## APM rules

- Keep `base.yml`'s top-level `targets`; it prevents deployment to unintended
  harnesses.
- Use dependency-level `targets` to narrow one package. Filters apply to the
  whole package, so split packages when primitive compatibility differs.
- `copilot-cowork` remains experimental and receives skills only. A
  Cowork-targeted skill must remain useful without MCP, hooks, prompts, agents,
  commands, or instructions.
- Keep Cowork's APM feature enablement and path precedence, but use the custom
  file-level reconciliation while upstream replaces destination directories.
  Lockfile `target_subset` values remain authoritative deployment policy.
- Keep self-defined MCP servers direct unless transitive MCP trust is deliberate.
- Local plugins use `apm.yml`, `includes: auto`, and `.apm/<primitive>/`.
- Reference local plugins with forward-slash paths such as
  `~/.apm/plugins/dot-agent`.
- New config fragments also need the matching `conf/symlinks.toml` entry.
- Never place secrets or private/employer content in public APM files.

## Engine rules

Dotfiles owns fragment merging, generated-manifest persistence, conditional
target orchestration, Cowork's ACL-safe exception, and the Copilot App autopilot
fixup. Native APM owns install/update planning, local-source integrity,
deployment convergence, and stale/orphan cleanup. Do not reintroduce
fingerprints, success markers, `apm outdated` parsing, or a separate
`apm prune`. Keep update-only scheduling in the command pipeline.

## Verify

After APM config or plugin changes run `./dotfiles.sh install -d`. For Rust or
agent-settings changes, also use `cross-platform-verification`.
