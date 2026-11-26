---
name: package-management
description: >
  Use for conf/packages.toml or package providers/resources under
  cli/src/domains/packages/, including pacman, AUR helpers, and winget. Not for
  generic subprocess or task orchestration changes, or dotfiles-managed PKGBUILD
  recipe and snapshot work (use arch-package-recipes).
---

# Package Management

See [Package management](../../../docs/content/guides/package-management.md)
for human workflows, [planning.rs](../../../cli/src/domains/packages/install/planning.rs)
for install planning, and [resources/](../../../cli/src/domains/packages/resources/)
for providers.

## Non-obvious invariants

- When choosing new packages in `conf/packages.toml`, check transitive
  dependencies and prefer lean tools that fit the existing Hyprland/Wayland
  stack over pulling in heavy unrelated desktop ecosystems.
- A missing native package manager for selected packages is `TaskResult::unmet`,
  not a benign skip. A failed installed-state query must propagate rather than
  returning an empty set (which would attempt to reinstall everything).
- `pacman` installs missing packages with `-Syu`, which also synchronizes
  databases and upgrades installed packages—do not treat or document it as
  add-only.
- Never wrap AUR helpers in an extra `sudo` layer. Check the PATH-selected
  `paru` binary's health, not just existence. Keep bootstrap/rebuild planning
  separate from AUR installs with a blocking AUR-to-Paru edge, and let Paru
  wait on native packages without inheriting unrelated package failures.
- AUR dry-run queries `pacman`'s database without requiring a `paru` binary that
  bootstrap only previewed.
- Consumer tasks may waive application-selected package ordering edges only
  after their `ordering_requirements()` and `package_stability_requirement`
  prove no package install or Paru rebuild is needed. Failed inventory is
  unknown readiness (retaining its cause), never proof of no work.
- Winget uses exact IDs, tries user scope first, and retries unscoped only when
  no user-scope installer exists. Privilege and policy refusals return
  `ResourceChange::unusable` (counted as failed items, not benign skips).
