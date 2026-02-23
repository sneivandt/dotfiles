# Integration Tests

This directory contains integration tests for the `dotfiles-cli` crate. Each
test file is compiled as a separate test binary and exercises the public API of
the library.

## Structure

```
tests/
├── common/
│   └── mod.rs              Shared helpers (IntegrationTestContext, TestContextBuilder)
├── install_command.rs      Tests for install task list and --skip/--only filtering
├── uninstall_command.rs    Tests for uninstall task list
├── test_command.rs         Tests for config loading and validation
├── fixtures/
│   ├── base_profile.ini    Minimal symlinks.ini for a base-profile test
│   └── desktop_profile.ini Minimal symlinks.ini for a desktop-profile test
├── snapshots/
│   ├── install_command__install_task_names.snap
│   └── uninstall_command__uninstall_task_names.snap
└── README.md               This file
```

## Running Integration Tests

```bash
# Run all tests (unit + integration)
cd cli
cargo test

# Run only integration tests
cargo test --test install_command
cargo test --test uninstall_command
cargo test --test test_command
```

## Snapshot Tests

Two tests use [insta](https://insta.rs) snapshot assertions to guard against
unintentional changes to the install and uninstall task lists:

- `install_command::install_task_names` — full ordered list of install tasks
- `uninstall_command::uninstall_task_names` — full ordered list of uninstall tasks

### Updating Snapshots

When you intentionally add, remove, or rename a task, you must update the
stored snapshots. The recommended workflow is:

```bash
# Accept all new/changed snapshots automatically
cd cli
INSTA_UPDATE=unseen cargo test

# Review changes interactively (requires `cargo install cargo-insta`)
cargo insta review
```

The updated `.snap` files in `tests/snapshots/` should be committed together
with the code change that caused them.

### Snapshot File Format

Snapshot files use a plain-text format: a YAML front-matter block followed by
the snapshot content:

```
---
source: tests/install_command.rs
expression: "task_names.join(\"\\n\")"
---
Enable developer mode
Configure sparse checkout
...
```

## Adding New Test Cases

1. Pick the appropriate test file based on the command being tested.
2. Write a `#[test]` function. Use `common::IntegrationTestContext::new()` for
   tests that need an isolated repository, or use `tasks::all_install_tasks()`
   directly for task-list assertions.
3. For snapshot tests, run `INSTA_UPDATE=unseen cargo test` once to create the
   initial snapshot, review it, then commit both the test and the `.snap` file.

### Using `IntegrationTestContext`

```rust
mod common;

#[test]
fn my_test() {
    let ctx = common::IntegrationTestContext::new();
    let config = ctx.load_config("base");
    // ... assertions ...
}
```

### Using `TestContextBuilder`

`TestContextBuilder` lets you override individual config files before building
the context:

```rust
#[test]
fn test_with_custom_symlinks() {
    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.ini", "[base]\nbashrc\n")
        .with_symlink_source("bashrc")   // creates the source file on disk
        .build();

    let config = ctx.load_config("base");
    assert_eq!(config.symlinks.len(), 1);
}
```

## Debugging Tips

- Add `RUST_BACKTRACE=1` before `cargo test` for full stack traces on failures.
- Use `cargo test <test_name>` to run a single test by name prefix.
- The `IntegrationTestContext` uses `tempfile::TempDir` for its temporary
  directories, which are automatically cleaned up when dropped. If you need
  to inspect a temp directory during debugging, add explicit logging of its
  path in your test code.
- Snapshot mismatches show a diff: the left side is the stored snapshot, the
  right side is the actual output. If the change is expected, update the
  snapshot as described above.
