---
name: ai-tooling-apm
description: >
  AI tooling and APM plugin workflow for this dotfiles repo. Use when changing
  Copilot/agent settings, APM dependencies, local APM plugins, or skill
  distribution through symlinks/apm.
---

# AI Tooling and APM

This repo provisions AI tooling as part of the dotfiles system. APM config is
managed under `symlinks/apm/`, linked into the home directory, then installed by
the Rust `InstallApmPackages` task.

## Moving Parts

| Path | Purpose |
|---|---|
| `symlinks/apm/config/base.yml` | APM manifest fragment linked to `~/.apm/config/base.yml` |
| `symlinks/apm/plugins/dot-code` | Local coding workflow skills |
| `symlinks/apm/plugins/dot-copilot` | Local Copilot interaction workflow skills |
| `symlinks/apm/plugins/dot-doc` | Local document-generation workflow skills |
| `symlinks/apm/plugins/dot-skill` | Local skill/plugin maintenance skills |
| `conf/symlinks.toml` | Links `apm/config/base.yml` and `apm/plugins/*` from this repo |
| `cli/src/tasks/ai/apm/` | `InstallApmPackages` (Provision phase) merges fragments + runs `apm install`; `UpdateApmPackages` (Update phase, `update` command only) advances locked deps via `apm outdated` + `apm update` (`mod.rs` orchestration; `fragments.rs`, `manifest.rs`, `outdated.rs`, `autopilot.rs`) |

## When to Change What

- Update `.github/skills/` for repository-specific coding patterns that should
  guide agents working on this dotfiles codebase.
- Update `symlinks/apm/plugins/dot-*` for personal reusable skills that should
  be installed into the user's global APM environment.
- Update `symlinks/apm/config/base.yml` when adding, removing, or re-grouping
  external APM plugin dependencies.
- Update docs when the user-facing install, Windows, or usage workflow changes.

## MCP Servers and Hooks via APM

APM owns more than skills. `merge_fragments` in `apm/fragments.rs` aggregates both
`dependencies.apm` and `dependencies.mcp` from every `~/.apm/config/*.yml`
fragment, so AI tooling can be delivered through APM instead of raw symlinks:

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
  `~/.copilot/copilot-instructions.md`. Keep repo-specific instructions in
  `.github/copilot-instructions.md`; use APM only for reusable user-scope
  instruction packages.

## Local Plugin Rules

- Keep local plugin names short and `dot-*`: `dot-code`, `dot-copilot`,
  `dot-doc`, `dot-skill`.
- Ensure each plugin folder has a matching `plugin.json` `name` field, and
  declare its skills explicitly with `"skills": ["skills/"]`.
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

`install` converges to the locked manifest and never advances locked refs:
the Configure-phase `InstallApmPackages` task only runs `apm install`.  To pull in
newer plugin/MCP dependency versions, run `./dotfiles.sh update`, which adds a
separate **Updating dependencies** phase whose `UpdateApmPackages` task runs
`apm outdated` + `apm update`.  That task guards itself — it only contacts
APM when the manifest has already been installed successfully (lockfile present
and the success marker matches) — so a failed/partial install never advances
locked refs.  The `update`-only scheduling lives in `run_pipeline`
(`commands/install.rs`); the task itself does not read `ctx.advance_versions`.

For changes to `cli/src/tasks/ai/apm/`, also run the Rust checks from the
`cross-platform-verification` skill.
