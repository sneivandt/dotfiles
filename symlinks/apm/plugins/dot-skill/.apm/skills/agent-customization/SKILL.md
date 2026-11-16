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
- Package reusable public content with APM. Group related skills, hooks,
  prompts, and instructions by purpose. Use configuration fragments to select
  packages and declare external dependencies.
- Put reusable task workflows in `dot-workflow` and skill authoring and
  maintenance guidance in `dot-skill`.
- Set model, reasoning, and UI defaults in `conf/agent-settings.toml`.
- Give each skill one recurring job. Extend an existing skill when it already
  covers the same intent.

## APM package rules

- Use native layout: `apm.yml`, `includes: auto`, and `.apm/<primitive>/`.
- Use short `dot-*` names for local plugins. Match each skill's directory to
  its frontmatter name.
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

Use the dotfiles checkout's `docs/content/development/testing.md` for scoped
checks and previews. For skill-text-only edits, review frontmatter, links, and
trigger descriptions.
For behavior changes, prefer current-source checks with isolated homes and
synthetic overlays; a wrapper may bootstrap or run an older installed binary.
An authorized preview with a real overlay can execute its cooperative
`--check`/`--dryrun` scripts. Inspect trusted scripts first and keep private paths
and output out of public reports.
