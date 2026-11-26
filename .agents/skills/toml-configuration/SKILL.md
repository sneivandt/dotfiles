---
name: toml-configuration
description: >
  Use when editing conf/*.toml data, typed models, loaders, category sections,
  public/overlay section merging, profile resolution/categories (profiles.rs),
  Config::validate diagnostics, check-command validation tasks, or
  cli/tests/config_drift.rs. Prefer the owning domain skill for data-only
  changes when one exists. Not for overlay path selection or script execution
  (use overlay-scripts).
---

# TOML Configuration, Profiles, and Validation

## Parsing and section merging

- Add `#[serde(deny_unknown_fields)]` to deserialized structs; use
  `crate::infra::config::StringOrTable<T>` instead of `#[serde(untagged)]`.
- Preserve the two-layer absence policy: `ConfigLoader::read_config` requires
  the main config inventory while reusable section loaders allow absent optional
  overlay files. Domain decoders consume the parsed `ConfigDocument` with
  preserved spans and never reopen files.
- Section headers are hyphen-separated category conjunctions (`[arch-desktop]`,
  never dotted). Built-in tags (`base`, `desktop`, `linux`, `windows`, `arch`,
  `wsl`) are owned by `KNOWN_CATEGORIES` in
  [profiles.rs](../../../cli/src/app/config/profiles.rs) and enforced by
  [preflight.rs](../../../cli/src/app/config/preflight.rs).
- Use `config_section!`, `SectionLoader`, and
  [`merge::OverlayEntry`](../../../cli/src/app/config/merge.rs) rather than
  hand-written merges: active overlay entries replace matching base identities
  (preserving base order, except system files where replacements keep their
  original position) and stamp source `origin`. Use `collect_views` when both
  active and unfiltered validation views are needed.

## Profile resolution

- Precedence: `--profile > DOTFILES_PROFILE > repository dotfiles.profile > interactive prompt`.
- Never silently default to `base` when no profile is configured. Non-interactive
  runs without a selection fail with guidance.
- `resolve_read_only()` must not prompt or write repository config; only an
  explicit `--profile` or interactive prompt selection persists. Keep profile
  definitions and active-category computation in
  [profiles.rs](../../../cli/src/app/config/profiles.rs) and `profiles/`.

## Semantic validation and drift checks

- Distinguish startup vs check semantics: loader/structural errors can abort
  startup, whereas semantic diagnostics from `Config::validate` fail
  `dotfiles check` even at warning severity without aborting ordinary mutation.
- Domain `validate()` functions belong in `cli/src/domains/*/config/` and are
  aggregated by `Config::validate`. Keep them read-only, deterministic, and
  focused on schema/data integrity (never run external tools or mutate host
  state).
- `Validator` helpers (`check`, `check_with_hint`, `require_non_empty`,
  `require_relative_path`, `require_within_root`) take an **invalid** predicate
  (`true` records a diagnostic). `reject_conflicts` checks key/name collisions
  only; validate individual items before deduplicating.
- Validate unfiltered cross-platform views (including inactive categories and
  both base and overlay sources) so disabled profiles cannot hide broken data.
- Keep real `conf/*.toml` and `symlinks/` aligned with
  [cli/tests/config_drift.rs](../../../cli/tests/config_drift.rs).

See [Testing](../../../docs/content/development/testing.md#choosing-coverage)
for TOML/validation test targets and
[Contributing](../../../docs/content/development/contributing.md#change-checklists)
for the full new-section checklist.
