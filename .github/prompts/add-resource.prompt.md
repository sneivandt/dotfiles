---
description: "Add a new resource type to the dotfiles engine — creates resource struct, trait impls, config loader, task, and test"
agent: "agent"
argument-hint: "Resource name, e.g. cron-job or dconf-setting"
---

Add a new resource type called `{{resource_name}}` to the dotfiles engine. Follow the project's
established patterns by completing these steps:

1. **Resource struct** — Create `cli/src/resources/{{resource_name}}.rs`
   - Implement `Applicable` (describe, apply, remove)
   - Implement `Resource` if the resource can check its own state; otherwise use bulk-checked pattern
   - Follow the templates in the `resource-implementation` skill

2. **Config loader** — Create `cli/src/config/{{resource_name}}.rs`
   - Use the `config_section!` macro
   - Support category-based section filtering
   - Follow the config loader pattern in the `toml-configuration` skill

3. **TOML file** — Create `conf/{{resource_name}}.toml` with at least a `[base]` section

4. **Task** — Create the task in `cli/src/phases/apply/`
   - Use the `resource_task!` macro
   - Register in `cli/src/phases/catalog.rs` (`all_install_tasks` and `all_uninstall_tasks`)

5. **Module wiring** — Add `pub mod` declarations in `cli/src/resources/mod.rs` and `cli/src/config/mod.rs`

6. **Tests** — Add a unit test for config loading and verify with:
   ```bash
   cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
   ```

Read the `resource-implementation`, `rust-patterns`, and `toml-configuration` skills before starting.
Never use `.unwrap()` or `.expect()` — use `?` with `anyhow::Result`.
