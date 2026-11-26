---
name: agent-customization
description: >
  Choose where reusable agent customization belongs in dotfiles, a repository,
  or a private overlay. Use for APM package structure and target compatibility.
---

# Agent Customization

## Place the change

- Keep guidance tied to a codebase in that repository's `.agents/skills/`.
- Put host-specific, employer-specific, or private guidance in a private overlay.
- Package reusable public content with APM (`dot-workflow` for task workflows,
  `dot-skill` for skill authoring/maintenance). Use configuration fragments to
  select packages and declare external dependencies.
- Set model, reasoning, and UI defaults in `conf/agent-settings.toml`.

## APM package rules

- Use native layout (`apm.yml`, `includes: auto`, `.apm/<primitive>/`), short
  `dot-*` local plugin names, forward-slash local package references, and skill
  directory names matching frontmatter `name`.
- Keep public files free of secrets, private URLs, and employer-confidential
  data; add private fragments/plugins via overlay symlinks and reference
  credentials through environment variables.
- Keep self-defined MCP servers direct unless transitive trust is intentional.

## Target compatibility

- Top-level `targets` selects default harnesses; a dependency-level `targets`
  list narrows the entire package (split packages when primitives need
  different target sets).
- Cowork is an experimental, skills-only target: any Cowork-targeted package's
  skills must work without its MCP servers, hooks, prompts, agents, commands,
  or instructions.
