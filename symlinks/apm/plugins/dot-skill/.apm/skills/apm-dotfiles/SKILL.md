---
name: apm-dotfiles
description: >
  Use when deciding where Stuart's reusable agent customization belongs:
  public dotfiles vs private overlay, dot-agent vs dot-skill, APM target
  compatibility, or plugin/config-fragment structure. Not for repo Rust details.
---

# APM Dotfiles

## Place the change

| Content | Owner |
|---|---|
| reusable interaction or coding workflow | `dot-agent` |
| skill/plugin authoring and maintenance | `dot-skill` |
| repository-specific guidance | that repository's `.agents/skills/` |
| work, host, employer, or private guidance | private overlay plugin |
| model/reasoning/UI defaults | public `conf/agent-settings.toml` |
| skills, hooks, prompts, instructions, MCP | APM plugin or fragment |

Prefer one narrow skill per recurring behavior. Update an existing skill instead
of adding a near-duplicate.

## Plugin rules

- Use native layout: `apm.yml`, `includes: auto`, and `.apm/<primitive>/`.
- Keep local package names short and `dot-*`.
- Reference local packages with forward slashes.
- Public files contain no secrets, private URLs, or employer-confidential data.
- Private overlays add fragments/plugins through overlay symlink config and
  reference credentials through environment variables.
- Keep self-defined MCP servers direct unless transitive trust is intentional.

## Target compatibility

- Top-level `targets` selects default harnesses.
- A dependency-level `targets` list narrows the entire package.
- Split packages when primitives need different target sets.
- Cowork remains an experimental, skills-only target. A Cowork-targeted
  package's skills must work without its MCP servers, hooks, prompts, agents,
  commands, or instructions.
- Dotfiles enables Cowork's APM feature and honors APM's configured path, then
  follows lockfile target subsets during ACL-safe file reconciliation.
  Dependency filters are authoritative.

## Verify

For public APM config or plugin changes run `./dotfiles.sh install -n`. Validate
private-overlay changes from a checkout where that overlay is configured. Use
the repository's `ai-tooling-apm` skill for Rust implementation details.
