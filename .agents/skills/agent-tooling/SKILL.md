---
name: agent-tooling
description: >
  Use when editing this repo's conf/agent-settings.toml, symlinks/apm/ files,
  or cli/src/domains/ai/ implementation (agent-settings and apm). Not for
  general skill-writing advice, audience/placement decisions, or unrelated
  agent behavior.
---

# Agent Tooling

See [Agent configuration](../../../docs/content/guides/agent-configuration.md)
for sources, fragment merging, targets, and preview limits.

## Non-obvious invariants

- Adding a new `symlinks/apm/config/*.yml` fragment requires a matching
  `conf/symlinks.toml` entry.
- Keep explicit base `targets`; dependency-level filters narrow whole packages,
  so split packages when primitives need different harnesses. Cowork receives
  skills only, so Cowork-targeted skills must work without MCP, hooks, prompts,
  or agents.
- Local plugins under `symlinks/apm/plugins/dot-*` use `apm.yml`,
  `includes: auto`, `.apm/<primitive>/`, and forward-slash references. Keep
  self-defined MCP dependencies direct unless transitive trust is deliberate.
- `agent-settings` manages harness keys independently while preserving unmanaged
  keys and exact JSON/TOML numeric fidelity (`cli/src/domains/ai/resources/agent_settings/`).
- `agent-packages` owns APM convergence (`apm` is a compatibility selector).
  Ref advancement belongs to the update pipeline, not ordinary install.
- Preserve native `update` followed by `install` before target adapters,
  including failure classification and workflow-policy restoration. Do not
  reintroduce separate prune, fingerprints, or outdated-output parsing.
- Before removing either compatibility adapter, inspect current upstream APM
  code and prove in isolated platform fixtures that the replacement preserves
  [Copilot workflow ownership](../../../cli/src/domains/ai/apm/autopilot/) and
  [Cowork file ownership](../../../cli/src/domains/ai/apm/cowork.rs). A version
  bump alone is insufficient.
- Native APM owns syntax/schema validation; dotfiles owns cross-file local
  source relationships in [validation.rs](../../../cli/src/domains/ai/apm/validation.rs)
  and [test_fixture.rs](../../../cli/src/domains/ai/apm/test_fixture.rs).
- Packing is not deployment proof; validate adapters with synthetic fragments,
  lockfiles, and target directories rather than deploying to the live home.
