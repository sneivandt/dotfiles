---
name: agent-tooling
description: >
  Use when editing this repo's conf/agent-settings.toml, symlinks/apm/ files,
  or cli/src/domains/ai/apm implementation. Not for general skill-writing
  advice, audience/placement decisions, or unrelated agent behavior.
---

# Agent Tooling

Start with [Agent tooling](../../../docs/AGENT-TOOLING.md) for deployment
ownership, fragment merging, target compatibility, and adapter behavior.

## Choose the owner

| Change | Location |
|---|---|
| repo-specific coding skill | `.agents/skills/` |
| reusable personal skill, hook, prompt, or instruction | `symlinks/apm/plugins/dot-*` |
| external plugin or MCP dependency | `symlinks/apm/config/*.yml` |
| stable Copilot/Codex preference | `conf/agent-settings.toml` |
| install and pin-update behavior | `cli/src/domains/ai/apm.rs` and `cli/src/domains/ai/apm/` |

The personal `agent-customization` skill is optional audience guidance;
repository work must remain possible without it.

## Decisions and pitfalls

- Edit source fragments/plugins, not generated manifests, deployed content, or
  lock data in the live home. Keep secrets and private content out of public files.
- Keep explicit base targets; dependency-level filters restrict whole packages.
  Split packages when primitive compatibility differs. Cowork receives skills
  only, so its skills must work without MCP, hooks, prompts, or agents.
- A new fragment needs its `conf/symlinks.toml` entry. Keep self-defined MCP
  dependencies direct unless transitive trust is deliberate. Local plugins use
  `apm.yml`, `includes: auto`, `.apm/<primitive>/`, and forward-slash references.
- `agent-settings` manages harness keys independently; `agent-packages` owns APM
  convergence (`apm` is a compatibility selector). Ref advancement belongs to
  the update pipeline, not an ordinary install.
- Preserve native update followed by install before target adapters, including
  failure classification and workflow-policy restoration. Do not reintroduce
  separate prune, fingerprints, or outdated-output parsing.
- Before removing either compatibility adapter, inspect current upstream code
  and prove the replacement against the
  [deployment contracts](../../../docs/AGENT-TOOLING.md#deployment-targets)
  in isolated fixtures on the relevant platforms. A version bump is insufficient.
- Native APM owns syntax/schema validation; dotfiles owns cross-file local
  source relationships. Extend
  [validation.rs](../../../cli/src/domains/ai/apm/validation.rs) and the
  [fixture](../../../cli/src/domains/ai/apm/test_fixture.rs) accordingly.

## Validate the changed contract

Use [CLI validation](../../../docs/TESTING.md#cli-validation) and
[Choosing coverage](../../../docs/TESTING.md#choosing-coverage).
Preview reconciliation follows [Dry-run testing](../../../docs/TESTING.md#dry-run-testing).
Packing is not deployment proof. Do not deploy to the live home to test content
or an adapter; use synthetic fragments, lockfiles, and target directories.
