---
title: Agent deployment maintenance
description: Change APM deployment implementation while preserving target filtering, workflow policy, ownership, and preview behavior.
---

# Agent deployment maintenance

This guide is for contributors changing agent deployment code. To change
preferences, add a skill, or deploy packages, use
[Agent configuration](agent-configuration.md).

The implementation lives under
[cli/src/domains/ai/apm/](https://github.com/sneivandt/dotfiles/tree/main/cli/src/domains/ai/apm/).
Native APM owns dependency resolution, local-source verification, and stale
deployment cleanup. Dotfiles combines configuration fragments and manages
the detected Copilot App and Cowork integrations.

## Deployment sequence

The task atomically writes the generated manifest when needed, rejecting a
symlink at that destination. It runs `apm update -g --yes`, then
`apm install -g`, before the target adapters. Preserve the install pass even
when refs are unchanged: local plugin edits or missing deployments can still
need applying. Do not add separate prune commands, success fingerprints, or
`apm outdated` decisions.

Preview must not install, repair targets, clean legacy locks, or write Cowork
inventories. Preserve authentication classification and attempt workflow-policy
restoration even after an earlier deployment phase fails.

## Find the implementation

| Owner | Contract to preserve |
|---|---|
| [Fragment merging](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/apm/fragments.rs) | Use effective source filenames and the [fragment merge rules](agent-configuration.md#fragment-merge-rules), including unmanaged home fragments |
| [Copilot workflow repair](https://github.com/sneivandt/dotfiles/tree/main/cli/src/domains/ai/apm/autopilot/) | Install with `apm install -g --target copilot-app --only apm`; repair only exact lockfile-owned workflow IDs, preserving foreign workflows |
| [Cowork path selection](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/apm/targets.rs) | A nonempty environment override wins; malformed persisted configuration fails rather than silently falling back |
| [Cowork copying](https://github.com/sneivandt/dotfiles/blob/main/cli/src/domains/ai/apm/cowork.rs) | Stage file updates without native Cowork deployment; preserve directory ACLs and foreign files, reject linked destination directories, and remove unchanged stale files recorded in the ownership inventory |

Keep workflow scheduling and duplicate cleanup limited to owned IDs. Cowork
updates must preserve directory permissions and foreign files, respect package
target filters, and retain inventory-based cleanup. The linked implementations
and tests contain the detailed schedule, legacy-lock, and path-repair rules.

## Verify an implementation change

Use isolated homes, synthetic App databases, controlled executors, and the
[Testing guide](testing.md#choosing-coverage). Never deploy to the live home to
test an implementation change. Dotfiles validates local source relationships;
native `apm pack --dry-run --verbose` checks package structure. Packing
does not prove deployment compatibility.

Before replacing an adapter, verify target filtering, ACL-safe updates,
owned stale cleanup, workflow-policy restoration, and preservation of foreign
workflows on the relevant platforms. Include failures after partial deployment.
A newer APM version alone does not prove that an adapter can be removed.
