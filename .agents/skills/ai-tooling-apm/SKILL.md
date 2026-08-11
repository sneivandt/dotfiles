---
name: ai-tooling-apm
description: >
  AI tooling and APM plugin workflow for this dotfiles repo. Use when changing
  Copilot/Codex/agent settings, APM dependencies, local APM plugins, or skill
  distribution through symlinks/apm.
---

# AI Tooling and APM

This repo provisions AI tooling as part of the dotfiles system. APM config is
managed under `symlinks/apm/`, linked into the home directory, then installed by
the Rust `InstallApmPackages` task.

## Moving Parts

| Path | Purpose |
|---|---|
| `symlinks/apm/config/*.yml` | APM manifest fragments merged by APM; `base.yml` is the baseline, platform-scoped fragments such as `arch.yml` layer on top |
| `symlinks/apm/plugins/dot-agent` | Local agent interaction workflow skills |
| `symlinks/apm/plugins/dot-skill` | Local skill/plugin maintenance skills |
| `conf/symlinks.toml` | Links `apm/plugins/*` and each `apm/config/*.yml` fragment from its matching category section (`base.yml` under `[base]`, `arch.yml` under `[arch]`) |
| `cli/src/domains/ai/apm.rs` + `cli/src/domains/ai/apm/` | APM install/update tasks and support code |

## When to Change What

- Update `.agents/skills/` for repository-specific coding patterns that should
  guide agents working on this dotfiles codebase.
- Update `symlinks/apm/plugins/dot-*` for personal reusable skills that should
  be installed into the user's global APM environment.
- Update the relevant `symlinks/apm/config/*.yml` fragment when adding,
  removing, or re-grouping external APM plugin dependencies: `base.yml` for
  everything cross-platform, a platform-scoped fragment (for example
  `arch.yml`) when the dependency only applies there. Keep `base.yml`'s
  top-level `targets:` list intact: it pins which runtimes APM deploys into, and
  dropping it lets APM auto-detect and deploy into unused harness directories. A
  new fragment also needs a `conf/symlinks.toml` entry in its category section.
  See `docs/APM.md`.
- Update docs when the user-facing install, Windows, or usage workflow changes.

## MCP Servers and Hooks via APM

APM owns more than skills. `merge_fragments` in `apm/fragments.rs` layers
manifest fields from every `~/.apm/config/*.yml` fragment and dependency-aware
merges both `dependencies` and `devDependencies`, so AI tooling can be delivered
through APM instead of raw symlinks:

- **MCP servers**: declare self-defined stdio/http servers under
  `dependencies.mcp:` in a fragment (`base.yml` or an overlay's fragment).
  `apm install -g` writes the per-client config (`~/.copilot/mcp-config.json`
  for Copilot). Keep self-defined MCP servers **direct** in the fragment unless
  intentionally opting into transitive MCP trust with APM's
  `--trust-transitive-mcp`; direct declarations keep the trust boundary explicit.
- **Hooks**: ship `*.json` hooks under a local plugin's `.apm/hooks/`. APM
  deploys them to `~/.copilot/hooks/<plugin>-<file>.json` at user scope. A
  sidecar script (e.g. a `.ps1` the hook invokes) can ride along as a skill
  asset; it lands at `~/.agents/skills/<skill>/`, so point the hook there.
- **Instructions**: supported at Copilot user scope in current APM. Instruction
  primitives can be delivered through APM and are concatenated into
  `~/.copilot/copilot-instructions.md`. Keep repo-specific shared instructions
  in `AGENTS.md`; use APM only for reusable user-scope instruction packages.

## Local Plugin Rules

- Keep local plugin names short and `dot-*`: `dot-agent`, `dot-skill`.
- Use native APM package layout for each local plugin: `apm.yml` at the plugin
  root and source primitives under `.apm/` (for example,
  `.apm/skills/<skill>/SKILL.md`). Set `includes: auto` unless a plugin needs a
  stricter include allow-list.
- Reference local plugins with forward slashes (`~/.apm/plugins/dot-foo`), even
  in Windows-only overlay fragments. APM normalizes `/` on Windows and the
  dotfiles validator only recognizes the forward-slash form.
- Keep skills concise and composable; prefer updating an existing related skill
  over adding a near-duplicate.
- Do not put secrets, tokens, private URLs, or employer-confidential content in
  skills, plugin manifests, or APM config.

## Validation

After APM config or local plugin changes, run:

```sh
./dotfiles.sh install -d
```

`install` converges to the lockfile without advancing refs. Fragment discovery
uses configured symlink sources plus unmanaged home fragments so dry-run sees
the post-symlink desired state even when a managed home link is missing. Its
dry-run emits planned work only when the merged manifest, lockfile, or success
marker requires convergence. `update` dry-run probes with non-mutating `apm
outdated -g` and emits planned work only for confirmed outdated dependencies;
apply mode may run `apm update`, but only after a successful installed state is
confirmed. Keep update-only scheduling in the command pipeline rather than
teaching the task to inspect command flags.

For changes to `cli/src/domains/ai/apm.rs` or `cli/src/domains/ai/apm/`, also
run the Rust checks from the `cross-platform-verification` skill.
