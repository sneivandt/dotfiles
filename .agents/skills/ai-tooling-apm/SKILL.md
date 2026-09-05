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
| install and pin-update behavior | `cli/src/domains/ai/apm.rs` and `cli/src/domains/ai/apm/` |

For placement and target behavior, start with [APM](../../../docs/APM.md).
If available, the personal `apm-dotfiles` skill adds public-vs-private audience
guidance; it is not a prerequisite for working in this repository.

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
- Edit versioned plugin/fragment sources, not deployed copies or generated
  manifests under the user's home. Local install and pin-update commands mutate
  that environment; do not run them merely to review skill content.

## Engine rules

Dotfiles owns fragment merging, generated-manifest persistence, conditional
target orchestration, Cowork's ACL-safe exception, and the Copilot App autopilot
fixup. Native APM owns install/update planning, local-source integrity,
deployment convergence, and stale/orphan cleanup. Do not reintroduce
fingerprints, success markers, `apm outdated` parsing, or a separate
`apm prune`. Keep update-only scheduling behind `dotfiles install --update-pins`
in the command pipeline.

Do not duplicate native APM schema validation in Rust. Dotfiles' validator owns
cross-file local-plugin/source relationships; native APM owns fragment/package
syntax. Consult [cross-file validation](../../../cli/src/domains/ai/apm/validation.rs)
and the existing [APM fixture](../../../cli/src/domains/ai/apm/test_fixture.rs)
before extending coverage.

## Validation

Use the APM checks under [CLI validation](../../../docs/TESTING.md#cli-validation)
and preview reconciliation as described in
[Dry-run testing](../../../docs/TESTING.md#dry-run-testing). For Rust or
agent-settings changes, select additional coverage from
[Choosing coverage](../../../docs/TESTING.md#choosing-coverage).
