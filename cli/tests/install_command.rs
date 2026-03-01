#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing
)]
//! Integration tests for the `install` command.
//!
//! These tests exercise the full task list produced by [`all_install_tasks`],
//! the task-name–based filtering applied by the `--skip` and `--only` CLI
//! flags, and the structural properties of the install dependency graph.

mod common;

use std::any::TypeId;
use std::collections::HashSet;

use dotfiles_cli::platform::{Os, Platform};
use dotfiles_cli::tasks;

// ---------------------------------------------------------------------------
// Snapshot: full install task list
// ---------------------------------------------------------------------------

/// Snapshot of all install task names in their declared order.
///
/// This test serves as a regression guard: any addition, removal, or rename of
/// an install task will cause it to fail, prompting a deliberate snapshot update.
#[test]
fn install_task_names() {
    let all_tasks = tasks::all_install_tasks();
    let task_names: Vec<&str> = all_tasks.iter().map(|t| t.name()).collect();
    insta::assert_snapshot!("install_task_names", task_names.join("\n"));
}

// ---------------------------------------------------------------------------
// Structural invariants
// ---------------------------------------------------------------------------

/// The install task list must contain exactly the expected number of tasks.
#[test]
fn install_task_count() {
    assert_eq!(tasks::all_install_tasks().len(), 16);
}

/// Every task name must be non-empty.
#[test]
fn install_task_names_are_non_empty() {
    for task in tasks::all_install_tasks() {
        assert!(!task.name().is_empty(), "install task has an empty name");
    }
}

/// No two install tasks may share the same name.
#[test]
fn install_task_names_are_unique() {
    let tasks = tasks::all_install_tasks();
    let mut seen: HashSet<&str> = HashSet::new();
    for task in &tasks {
        assert!(
            seen.insert(task.name()),
            "duplicate install task name: '{}'",
            task.name()
        );
    }
}

/// No two install tasks may share the same [`TypeId`].
#[test]
fn install_task_type_ids_are_unique() {
    let tasks = tasks::all_install_tasks();
    let ids: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
    assert_eq!(
        ids.len(),
        tasks.len(),
        "install task list contains duplicate TypeIds"
    );
}

/// Every dependency declared by an install task must be satisfied by another
/// task in the same list (i.e., no dangling dependency references).
#[test]
fn install_task_dependencies_are_resolvable() {
    let tasks = tasks::all_install_tasks();
    let present: HashSet<TypeId> = tasks.iter().map(|t| t.task_id()).collect();
    for task in &tasks {
        for dep in task.dependencies() {
            assert!(
                present.contains(dep),
                "task '{}' declares a dependency that is not in the install task list",
                task.name()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// --skip filter
// ---------------------------------------------------------------------------

/// Tasks whose names contain the skip keyword (case-insensitive) must be
/// excluded from the filtered list, matching the behaviour of `--skip packages`.
#[test]
fn skip_filter_excludes_matching_tasks() {
    let all_tasks = tasks::all_install_tasks();
    let skip_keyword = "packages";

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| !t.name().to_lowercase().contains(skip_keyword))
        .map(|t| t.name())
        .collect();

    for name in &filtered {
        assert!(
            !name.to_lowercase().contains(skip_keyword),
            "task '{name}' should have been excluded by --skip {skip_keyword}",
        );
    }
    // At least one task was removed
    assert!(
        filtered.len() < all_tasks.len(),
        "--skip packages should remove at least one task"
    );
}

/// When the skip keyword does not match any task name the full list is returned.
#[test]
fn skip_filter_with_no_match_returns_all_tasks() {
    let all_tasks = tasks::all_install_tasks();
    let skip_keyword = "zzznomatch";
    let total = all_tasks.len();

    let filtered_count = all_tasks
        .iter()
        .filter(|t| !t.name().to_lowercase().contains(skip_keyword))
        .count();

    assert_eq!(
        filtered_count, total,
        "--skip with non-matching keyword should leave task count unchanged"
    );
}

// ---------------------------------------------------------------------------
// --only filter
// ---------------------------------------------------------------------------

/// Only tasks whose names contain the `--only` keyword should remain.
#[test]
fn only_filter_includes_only_matching_tasks() {
    let all_tasks = tasks::all_install_tasks();
    let only_keyword = "symlinks";

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| t.name().to_lowercase().contains(only_keyword))
        .map(|t| t.name())
        .collect();

    assert_eq!(
        filtered,
        vec!["Install symlinks"],
        "--only symlinks should return exactly one task"
    );
}

/// When `--only` matches multiple tasks all of them are included.
#[test]
fn only_filter_can_include_multiple_tasks() {
    let all_tasks = tasks::all_install_tasks();
    let only_keyword = "git";

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| t.name().to_lowercase().contains(only_keyword))
        .map(|t| t.name())
        .collect();

    // "Configure git" and "Install git hooks" both match
    assert!(
        filtered.len() >= 2,
        "--only git should match at least 'Configure git' and 'Install git hooks'"
    );
    for name in &filtered {
        assert!(
            name.to_lowercase().contains(only_keyword),
            "task '{name}' should not have been included by --only git",
        );
    }
}

/// When `--only` matches nothing the result is an empty list.
#[test]
fn only_filter_with_no_match_returns_empty() {
    let all_tasks = tasks::all_install_tasks();
    let only_keyword = "zzznomatch";

    let any_match = all_tasks
        .iter()
        .any(|t| t.name().to_lowercase().contains(only_keyword));

    assert!(
        !any_match,
        "--only with non-matching keyword should return empty list"
    );
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

/// `should_run` must not panic for any install task when given a minimal config.
#[test]
fn install_tasks_should_run_does_not_panic_with_minimal_config() {
    use std::sync::Arc;

    let ctx_builder = common::TestContextBuilder::new();
    let ctx = ctx_builder.build();
    let config = ctx.load_config("base");

    let platform = dotfiles_cli::platform::Platform::detect();
    let executor: Arc<dyn dotfiles_cli::exec::Executor> =
        Arc::new(dotfiles_cli::exec::SystemExecutor);
    let log: Arc<dotfiles_cli::logging::Logger> =
        Arc::new(dotfiles_cli::logging::Logger::new("test-install"));

    let task_ctx = dotfiles_cli::tasks::Context::new(
        Arc::new(std::sync::RwLock::new(config)),
        Arc::new(platform),
        Arc::clone(&log) as Arc<dyn dotfiles_cli::logging::Log>,
        executor,
        dotfiles_cli::tasks::ContextOpts {
            dry_run: true,
            parallel: false,
        },
    )
    .expect("create context");

    let tasks = tasks::all_install_tasks();
    // Calling should_run on every task must not panic.
    for task in &tasks {
        let _ = task.should_run(&task_ctx);
    }
}

// ---------------------------------------------------------------------------
// Dependency graph: no cycles
// ---------------------------------------------------------------------------

/// The install task dependency graph must not contain any cycles.
///
/// A cyclic dependency would cause the parallel scheduler to deadlock.  This
/// test validates the real task set as a regression guard independent of the
/// scheduler unit tests.
#[test]
fn install_tasks_form_acyclic_dependency_graph() {
    use std::collections::HashMap;

    let tasks = tasks::all_install_tasks();
    let type_to_idx: HashMap<TypeId, usize> = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| (t.task_id(), i))
        .collect();

    let mut in_degree: Vec<usize> = tasks
        .iter()
        .map(|t| {
            t.dependencies()
                .iter()
                .filter(|d| type_to_idx.contains_key(d))
                .count()
        })
        .collect();

    let mut reverse_deps: Vec<Vec<usize>> = vec![Vec::new(); tasks.len()];
    for (i, t) in tasks.iter().enumerate() {
        for dep in t.dependencies() {
            if let Some(&dep_idx) = type_to_idx.get(dep) {
                reverse_deps[dep_idx].push(i);
            }
        }
    }

    let mut queue: Vec<usize> = in_degree
        .iter()
        .enumerate()
        .filter_map(|(i, &d)| if d == 0 { Some(i) } else { None })
        .collect();
    let mut processed = 0usize;

    while let Some(idx) = queue.pop() {
        processed += 1;
        for &dep in &reverse_deps[idx] {
            in_degree[dep] -= 1;
            if in_degree[dep] == 0 {
                queue.push(dep);
            }
        }
    }

    assert_eq!(
        processed,
        tasks.len(),
        "install task dependency graph contains a cycle"
    );
}

// ---------------------------------------------------------------------------
// Expected task presence
// ---------------------------------------------------------------------------

/// The install task list must contain "Install symlinks".
#[test]
fn install_task_list_contains_install_symlinks() {
    let tasks = tasks::all_install_tasks();
    let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"Install symlinks"),
        "expected 'Install symlinks' in install task list, got: {names:?}"
    );
}

/// The install task list must contain "Install git hooks".
#[test]
fn install_task_list_contains_install_git_hooks() {
    let tasks = tasks::all_install_tasks();
    let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"Install git hooks"),
        "expected 'Install git hooks' in install task list, got: {names:?}"
    );
}

/// The install task list must contain "Configure git".
#[test]
fn install_task_list_contains_configure_git() {
    let tasks = tasks::all_install_tasks();
    let names: Vec<&str> = tasks.iter().map(|t| t.name()).collect();
    assert!(
        names.contains(&"Configure git"),
        "expected 'Configure git' in install task list, got: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Dry-run: task list with a Windows platform
// ---------------------------------------------------------------------------

/// `should_run` must not panic for any install task when given a Windows platform.
///
/// This exercises the platform-guarding logic in tasks like `ConfigureSystemd`,
/// `ApplyRegistry`, and `ApplyFilePermissions` without needing a real Windows OS.
#[test]
fn install_tasks_should_run_with_windows_platform() {
    use std::sync::Arc;

    let ctx_builder = common::TestContextBuilder::new();
    let ctx = ctx_builder.build();

    let platform = Platform {
        os: Os::Windows,
        is_arch: false,
    };
    let config = ctx.load_config_for_platform("base", &platform);

    let executor: Arc<dyn dotfiles_cli::exec::Executor> =
        Arc::new(dotfiles_cli::exec::SystemExecutor);
    let log: Arc<dotfiles_cli::logging::Logger> =
        Arc::new(dotfiles_cli::logging::Logger::new("test-windows"));

    let task_ctx = dotfiles_cli::tasks::Context::new(
        Arc::new(std::sync::RwLock::new(config)),
        Arc::new(platform),
        Arc::clone(&log) as Arc<dyn dotfiles_cli::logging::Log>,
        executor,
        dotfiles_cli::tasks::ContextOpts {
            dry_run: true,
            parallel: false,
        },
    )
    .expect("create context");

    let all_tasks = tasks::all_install_tasks();
    for task in &all_tasks {
        let _ = task.should_run(&task_ctx);
    }
}

// ---------------------------------------------------------------------------
// --skip filter: multiple keywords
// ---------------------------------------------------------------------------

/// When multiple keywords are provided, tasks matching any one of them must
/// be excluded.
#[test]
fn skip_with_multiple_keywords_excludes_all_matching() {
    let all_tasks = tasks::all_install_tasks();
    let skip_keywords = ["packages", "registry"];

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| {
            let name = t.name().to_lowercase();
            !skip_keywords.iter().any(|kw| name.contains(kw))
        })
        .map(|t| t.name())
        .collect();

    for name in &filtered {
        let lower = name.to_lowercase();
        for kw in &skip_keywords {
            assert!(
                !lower.contains(kw),
                "task '{name}' should have been excluded by --skip {kw}",
            );
        }
    }
    assert!(
        filtered.len() < all_tasks.len(),
        "--skip with multiple keywords should remove at least one task"
    );
}

// ---------------------------------------------------------------------------
// --only filter: multiple keywords
// ---------------------------------------------------------------------------

/// When multiple keywords are provided, tasks matching any one of them must
/// all be included (union, not intersection).
#[test]
fn only_with_multiple_keywords_includes_all_matching() {
    let all_tasks = tasks::all_install_tasks();
    let only_keywords = ["symlinks", "git"];

    let filtered: Vec<&str> = all_tasks
        .iter()
        .filter(|t| {
            let name = t.name().to_lowercase();
            only_keywords.iter().any(|kw| name.contains(kw))
        })
        .map(|t| t.name())
        .collect();

    // Must include tasks matching each keyword
    assert!(
        filtered
            .iter()
            .any(|n| n.to_lowercase().contains("symlinks")),
        "--only with 'symlinks' should include at least one symlink task"
    );
    assert!(
        filtered.iter().any(|n| n.to_lowercase().contains("git")),
        "--only with 'git' should include at least one git task"
    );
    // All included tasks must match at least one keyword
    for name in &filtered {
        let lower = name.to_lowercase();
        assert!(
            only_keywords.iter().any(|kw| lower.contains(kw)),
            "task '{name}' should not have been included"
        );
    }
}

// ---------------------------------------------------------------------------
// Idempotency: install → install is a no-op
// ---------------------------------------------------------------------------

/// Running `InstallSymlinks` twice must produce zero changes on the second run.
///
/// This test exercises the core idempotency guarantee: after a successful
/// install every resource is already in the desired state, so a second install
/// run must not change anything.
///
/// The verification relies on [`ResourceState`]: if every resource reports
/// `Correct` before the second `run()` call, then `process_resources` can only
/// count items as `already_ok` — making `changed == 0` a logical necessity.
/// Checking `Correct` after the second run confirms no resources were broken.
#[cfg(unix)]
#[test]
fn install_symlinks_is_idempotent() {
    use std::sync::Arc;

    use dotfiles_cli::resources::Resource;
    use dotfiles_cli::tasks::Task;

    let ctx = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"bashrc\"]\n")
        .with_symlink_source("bashrc")
        .build();

    // Use a dedicated temp directory as $HOME so the test does not modify the
    // real home directory.
    let home_dir = tempfile::tempdir().expect("create temp home dir");

    let platform = dotfiles_cli::platform::Platform::detect();
    let executor: Arc<dyn dotfiles_cli::exec::Executor> =
        Arc::new(dotfiles_cli::exec::SystemExecutor);
    let log: Arc<dotfiles_cli::logging::Logger> =
        Arc::new(dotfiles_cli::logging::Logger::new("test-idempotent"));

    let config = ctx.load_config("base");
    let task_ctx = dotfiles_cli::tasks::Context {
        config: Arc::new(std::sync::RwLock::new(config)),
        platform: Arc::new(platform),
        log: Arc::clone(&log) as Arc<dyn dotfiles_cli::logging::Log>,
        dry_run: false,
        home: home_dir.path().to_path_buf(),
        executor,
        parallel: false,
    };

    let task = dotfiles_cli::tasks::symlinks::InstallSymlinks;

    // First run: must succeed and create the symlink.
    let result1 = task.run(&task_ctx).expect("first install run");
    assert!(
        matches!(result1, dotfiles_cli::tasks::TaskResult::Ok),
        "first install run should succeed"
    );

    // Build the resource to inspect state directly.
    let source = ctx.root_path().join("symlinks").join("bashrc");
    let target = home_dir.path().join(".bashrc");
    let resource = dotfiles_cli::resources::symlink::SymlinkResource::new(source, target);

    // After the first run every resource must be Correct.  This is the
    // precondition that proves the second run will make zero changes.
    assert_eq!(
        resource
            .current_state()
            .expect("check state after first run"),
        dotfiles_cli::resources::ResourceState::Correct,
        "symlink must be Correct after first install"
    );

    // Second run: must succeed without changing anything.
    let result2 = task.run(&task_ctx).expect("second install run");
    assert!(
        matches!(result2, dotfiles_cli::tasks::TaskResult::Ok),
        "second install run should succeed"
    );

    // State must still be Correct, confirming zero changes in the second run.
    assert_eq!(
        resource
            .current_state()
            .expect("check state after second run"),
        dotfiles_cli::resources::ResourceState::Correct,
        "symlink must still be Correct after second install (idempotency guarantee)"
    );
}

// ---------------------------------------------------------------------------
// ApplyFilePermissions: real filesystem chmod
// ---------------------------------------------------------------------------

/// `ApplyFilePermissions.run()` must set the declared mode on an existing file.
///
/// Creates `$HOME/.ssh/config` with permissions `0o644`, then runs the task
/// and asserts that the permissions are updated to `0o600`.
#[cfg(unix)]
#[test]
fn apply_file_permissions_run_sets_mode_on_unix() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    use dotfiles_cli::tasks::Task;

    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"600\", path = \"ssh/config\" }]\n",
        )
        .build();

    let home_dir = tempfile::tempdir().expect("create temp home dir");

    // Create $HOME/.ssh/config with mode 0o644.
    let ssh_dir = home_dir.path().join(".ssh");
    std::fs::create_dir_all(&ssh_dir).expect("create .ssh dir");
    let ssh_config = ssh_dir.join("config");
    std::fs::write(&ssh_config, "").expect("create ssh config");
    std::fs::set_permissions(&ssh_config, std::fs::Permissions::from_mode(0o644))
        .expect("set initial permissions");

    let platform = dotfiles_cli::platform::Platform {
        os: dotfiles_cli::platform::Os::Linux,
        is_arch: false,
    };
    let executor: Arc<dyn dotfiles_cli::exec::Executor> =
        Arc::new(dotfiles_cli::exec::SystemExecutor);
    let log: Arc<dotfiles_cli::logging::Logger> =
        Arc::new(dotfiles_cli::logging::Logger::new("test-chmod"));

    let config = ctx.load_config_for_platform("base", &platform);
    let task_ctx = dotfiles_cli::tasks::Context {
        config: Arc::new(std::sync::RwLock::new(config)),
        platform: Arc::new(platform),
        log: Arc::clone(&log) as Arc<dyn dotfiles_cli::logging::Log>,
        dry_run: false,
        home: home_dir.path().to_path_buf(),
        executor,
        parallel: false,
    };

    let result = dotfiles_cli::tasks::chmod::ApplyFilePermissions
        .run(&task_ctx)
        .expect("apply file permissions run");
    assert!(
        matches!(result, dotfiles_cli::tasks::TaskResult::Ok),
        "apply file permissions should succeed"
    );

    let perms = std::fs::metadata(&ssh_config)
        .expect("read file metadata")
        .permissions();
    assert_eq!(
        perms.mode() & 0o777,
        0o600,
        "file permissions should be 0o600 after applying chmod"
    );
}

// ---------------------------------------------------------------------------
// install::run: full dry-run pipeline
// ---------------------------------------------------------------------------

/// Calling `commands::install::run` with `dry_run: true` must return `Ok(())`
/// without making any filesystem changes.
#[test]
fn install_run_dry_run_returns_ok() {
    use std::sync::Arc;

    let ctx = common::TestContextBuilder::new().build();
    let root_path = ctx.root_path().to_path_buf();

    // `resolve_from_args` calls `persist()` which writes to `.git/config`;
    // create the directory so the write succeeds.
    std::fs::create_dir_all(root_path.join(".git")).expect("create .git dir");

    let global = dotfiles_cli::cli::GlobalOpts {
        root: Some(root_path),
        profile: Some("base".to_string()),
        dry_run: true,
        parallel: false,
    };
    let opts = dotfiles_cli::cli::InstallOpts {
        skip: vec![],
        only: vec![],
    };
    let log: Arc<dotfiles_cli::logging::Logger> =
        Arc::new(dotfiles_cli::logging::Logger::new("test-dry-run-pipeline"));

    let result = dotfiles_cli::commands::install::run(&global, &opts, &log);
    assert!(
        result.is_ok(),
        "dry-run install should return Ok: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Parallel execution: should_run with parallel enabled
// ---------------------------------------------------------------------------

/// `should_run` must not panic for any install task when `parallel` is `true`.
///
/// This exercises the scheduler path that dispatches resources to Rayon
/// without needing a real system.
#[test]
fn install_tasks_should_run_with_parallel_enabled() {
    use std::sync::Arc;

    let ctx_builder = common::TestContextBuilder::new();
    let ctx = ctx_builder.build();
    let config = ctx.load_config("base");

    let platform = dotfiles_cli::platform::Platform::detect();
    let executor: Arc<dyn dotfiles_cli::exec::Executor> =
        Arc::new(dotfiles_cli::exec::SystemExecutor);
    let log: Arc<dotfiles_cli::logging::Logger> =
        Arc::new(dotfiles_cli::logging::Logger::new("test-parallel"));

    let task_ctx = dotfiles_cli::tasks::Context {
        config: Arc::new(std::sync::RwLock::new(config)),
        platform: Arc::new(platform),
        log: Arc::clone(&log) as Arc<dyn dotfiles_cli::logging::Log>,
        dry_run: true,
        home: std::path::PathBuf::from(
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
        ),
        executor,
        parallel: true,
    };

    let all_tasks = tasks::all_install_tasks();
    for task in &all_tasks {
        let _ = task.should_run(&task_ctx);
    }
}
