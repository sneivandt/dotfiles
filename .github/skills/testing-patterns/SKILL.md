---
name: testing-patterns
description: >
  Testing conventions and validation patterns for the dotfiles project.
  Use when creating tests, running validation, or setting up CI/CD.
metadata:
  author: sneivandt
  version: "2.0"
---

# Testing Patterns

The project uses Rust's built-in test framework, cargo clippy, and cargo fmt.

## Running Tests

```bash
cargo test                      # All unit + integration tests
cargo clippy -- -D warnings     # Lint check
cargo fmt -- --check            # Format check
./dotfiles.sh test              # Config validation via Rust engine
```

## Unit Tests

Every module has inline tests with `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        let result = my_function("input");
        assert_eq!(result, "expected");
    }
}
```

### Testing by Module Type

**Config parsers** — use `parse_sections_from_str()` to avoid file I/O:
```rust
#[test]
fn parse_simple_section() {
    let sections = parse_sections_from_str("[base]\nitem1\n").unwrap();
    assert_eq!(sections[0].items, vec!["item1"]);
}
```

**Tasks** — test helper functions (e.g., `compute_target()`):
```rust
#[test]
fn target_for_config() {
    assert_eq!(compute_target(&PathBuf::from("/home/u"), "config/git/config"),
               PathBuf::from("/home/u/.config/git/config"));
}
```

**Resources** — construct with `SystemExecutor` for unit tests that don't
need mocking. Resources that shell out take `&dyn Executor`:
```rust
#[test]
fn description_includes_manager() {
    let executor = crate::exec::SystemExecutor;
    let resource = PackageResource::new("git".to_string(), PackageManager::Pacman, &executor);
    assert_eq!(resource.description(), "git (pacman)");
}

#[test]
fn from_entry_copies_name() {
    let executor = crate::exec::SystemExecutor;
    let entry = SystemdUnit { name: "dunst.service".to_string() };
    let resource = SystemdUnitResource::from_entry(&entry, &executor);
    assert_eq!(resource.name, "dunst.service");
}
```

Resources that only do filesystem operations (e.g., `SymlinkResource`) do not
need an executor.

**Tasks** — use helpers from `cli/src/tasks/mod.rs` (in `#[cfg(test)]` scope):
```rust
// Context helpers
make_linux_context(config)           // Linux context
make_arch_context(config)            // Arch Linux context
make_windows_context(config)         // Windows context
make_platform_context(config, os, is_arch) // custom OS/arch combo
make_platform_context_with_which(config, os, is_arch, which_result) // control which()

// Config helper
empty_config(root_path)              // Config with all empty vecs

// Filesystem injection (for tasks that hold their own fs_ops field)
// Use the task's own with_fs_ops constructor, not a Context method:
let task = InstallGitHooks::with_fs_ops(Arc::new(MockFileSystemOps::new()
    .with_existing("/path")          // path returns true for exists()
    .with_file("/path/file")         // path returns true for is_file()
    .with_dir_entries("/dir", vec![..])  // read_dir returns given paths
));
```

**CLI** — test parsing with `Cli::parse_from()`:
```rust
#[test]
fn parse_dry_run() {
    let cli = Cli::parse_from(["dotfiles", "--dry-run", "install"]);
    assert!(cli.global.dry_run);
}
```

**Platform** — use `Platform::new()` to control detection (test-only API, `#[cfg(test)]`):
```rust
#[test]
fn excludes_windows_on_linux() {
    assert!(Platform::new(Os::Linux, false).excludes_category("windows"));
}
```

## Integration Tests

The `cli/tests/` directory contains integration test binaries:
- `install_command.rs` — verifies install task list via snapshot
- `uninstall_command.rs` — verifies uninstall task list via snapshot
- `test_command.rs` — verifies config validation

Integration tests use helpers from `cli/tests/common/mod.rs`:
- `IntegrationTestContext::new()` — sets up a temp-dir-backed repo clone
- `TestContextBuilder` — builder for custom repo layouts

Snapshot tests use the `insta` crate. Update snapshots with:
```bash
INSTA_UPDATE=unseen cargo test  # auto-accept new snapshots
cargo insta review              # interactive review
```
Commit `.snap` files alongside code changes.

## CI/CD

GitHub Actions: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt -- --check`, dry-run profile tests on Linux/Windows. Config validation via `./dotfiles.sh test`.

## Rules

1. Every new module must include `#[cfg(test)] mod tests`
2. Test pure functions; use `Platform::new()` and string parsers to avoid I/O
3. Use `SystemExecutor` when constructing resources in tests that only check descriptions or static state
4. Run `cargo clippy` and `cargo fmt` before committing
