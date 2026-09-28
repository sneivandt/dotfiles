#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests for the `install` command.
//!
//! These tests exercise the full task list produced by [`all_install_tasks`],
//! the task-selector filtering applied by the `--skip` and `--only` CLI
//! flags, and the structural properties of the install dependency graph.

mod common;

use dotfiles_cli::testing as test_api;

use test_api::config::ConfigStore;
use test_api::platform::{Os, Platform};
use test_api::tasks;
use test_api::tasks::filter::task_matches_filter;

/// Build an install task list backed by a store loaded from a minimal repo.
fn install_tasks() -> Vec<Box<dyn tasks::Task>> {
    install_tasks_for_platform(Platform::detect())
}

fn install_tasks_for_platform(platform: Platform) -> Vec<Box<dyn tasks::Task>> {
    let ctx = common::IntegrationTestContext::new();
    let store = ConfigStore::from_config(ctx.load_config_for_platform("base", platform));
    tasks::all_install_tasks(&store)
}

#[cfg(unix)]
#[test]
fn install_console_separates_tasks_and_keeps_no_op_compact() {
    use std::os::unix::fs::PermissionsExt as _;

    for verbose in [false, true] {
        for symbols in [false, true] {
            let repo = common::TestContextBuilder::new()
                .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"example\"]\n")
                .with_symlink_source_content("example", "example\n")
                .with_config_file(
                    "chmod.toml",
                    "[base]\npermissions = [{ path = \"example\", mode = \"600\" }]\n",
                )
                .build();
            std::fs::set_permissions(
                repo.root_path().join("symlinks/example"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            let home = tempfile::tempdir().unwrap();
            let overlay = tempfile::tempdir().unwrap();
            let mut command = common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "install",
                "symlinks,file-permissions",
            );
            if verbose {
                command.arg("--verbose");
            }
            if !symbols {
                command.arg("--no-symbols");
            }

            // Both runs mutate only the temporary repository and home.
            // The second run exercises hidden current rows and verbose rows.
            for changed in [true, false] {
                let output = command.output().expect("run isolated install");
                let text = String::from_utf8(output.stdout).unwrap();
                assert!(
                    output.status.success(),
                    "{text}\n{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    !text.contains('\u{1b}'),
                    "piped output must be plain: {text:?}"
                );
                assert!(!text.contains("\n\n\n"), "extra blank line: {text:?}");
                let blocks: Vec<_> = text.trim().split("\n\n").collect();
                assert_eq!(
                    blocks.len(),
                    if changed || verbose { 4 } else { 2 },
                    "{text}"
                );
                if changed || verbose {
                    let status = match (changed, symbols) {
                        (true, true) => "✓",
                        (true, false) => "CHANGE",
                        (false, true) => "○",
                        (false, false) => "OK",
                    };
                    for (block, name) in blocks[1..3]
                        .iter()
                        .zip(["Home symlinks", "File permissions"])
                    {
                        assert!(block.starts_with(&format!("{status} {name}")), "{text}");
                    }
                }
                assert!(
                    blocks.last().unwrap().starts_with(if changed {
                        "2 changed"
                    } else {
                        "No changes · 2 current"
                    }),
                    "{text}"
                );
            }
        }
    }
}

#[test]
fn conflicting_desired_state_stops_install_before_selected_tasks_run() {
    let mut cases = vec![(
        "git-config.toml",
        "[base]\nsettings = [{ key = \"core.editor\", value = \"vim\" }]\n",
        "[base]\nsettings = [{ key = \"CORE.EDITOR\", value = \"nano\" }]\n",
        "git.conflicting-values",
    )];
    if cfg!(windows) {
        cases.push((
            "registry.toml",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n",
            "[console]\npath = 'hkcu:\\console'\n[console.values]\nfontsize = 15\n",
            "registry.conflicting-values",
        ));
    }
    for (file, main, overlay_content, code) in cases {
        for dry_run in [false, true] {
            let repo = common::TestContextBuilder::new()
                .with_config_file(file, main)
                .with_config_file(
                    "symlinks.toml",
                    "[base]\nsymlinks = [\"conflict-sentinel\"]\n",
                )
                .with_symlink_source_content("conflict-sentinel", "must not be installed")
                .build();
            let overlay = tempfile::tempdir().unwrap();
            let home = tempfile::tempdir().unwrap();
            std::fs::create_dir(overlay.path().join("conf")).unwrap();
            std::fs::write(overlay.path().join("conf").join(file), overlay_content).unwrap();
            let mut command = common::cli_command(
                repo.root_path(),
                home.path(),
                Some(overlay.path()),
                "install",
                "symlinks",
            );
            if dry_run {
                command.arg("--dry-run");
            }
            let output = command.output().expect("run isolated install");
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                !output.status.success(),
                "{file}, dry_run={dry_run}: {text}"
            );
            assert!(text.contains(code), "{file}, dry_run={dry_run}: {text}");
            // The CLI canonicalizes --root, expanding Windows short path names.
            let root = dunce::canonicalize(repo.root_path()).expect("canonicalize repository root");
            assert!(
                text.contains(&root.join("conf").join(file).display().to_string()),
                "{text}"
            );
            assert!(
                text.contains(&overlay.path().join("conf").join(file).display().to_string()),
                "{text}"
            );
            assert!(
                home.path()
                    .join(".conflict-sentinel")
                    .symlink_metadata()
                    .is_err(),
                "even an unrelated selected task must not mutate before validation succeeds"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Structural invariants
// ---------------------------------------------------------------------------

#[test]
fn install_task_catalog_satisfies_structural_contract() {
    let tasks = install_tasks();
    common::assert_task_catalog_contract("install", &tasks);
}

// ---------------------------------------------------------------------------
// Task-selector filtering
// ---------------------------------------------------------------------------

#[test]
fn skip_filters_exclude_any_matching_selector_and_preserve_nonmatches() {
    let all_tasks = install_tasks();
    let cases: &[(&str, &[&str], bool)] = &[
        ("single selector", &["packages"], true),
        ("multiple selectors", &["packages", "registry"], true),
        ("no match", &["zzznomatch"], false),
    ];
    for (case, selectors, removes_tasks) in cases {
        let filtered: Vec<_> = all_tasks
            .iter()
            .filter(|task| {
                !selectors
                    .iter()
                    .any(|selector| task_matches_filter(task.as_ref(), selector))
            })
            .collect();
        for task in &filtered {
            for selector in *selectors {
                assert!(
                    !task_matches_filter(task.as_ref(), selector),
                    "{case}: '{}' should be excluded by --skip {selector}",
                    task.name()
                );
            }
        }
        if *removes_tasks {
            assert!(
                filtered.len() < all_tasks.len(),
                "{case}: nothing was removed"
            );
        } else {
            assert_eq!(filtered.len(), all_tasks.len(), "{case}");
        }
    }
}

#[test]
fn only_filters_use_exact_selectors_and_union_multiple_matches() {
    let all_tasks = install_tasks();
    let cases: &[(&str, &[&str], &[&str])] = &[
        ("single selector", &["symlinks"], &["Home symlinks"]),
        (
            "repository selector",
            &["repository"],
            &["Dotfiles repository"],
        ),
        ("ambiguous update label", &["update"], &[]),
        ("internal report label", &["report"], &[]),
        ("unknown selector", &["zzznomatch"], &[]),
        (
            "union",
            &["symlinks", "git-hooks"],
            &["Git hooks", "Home symlinks"],
        ),
    ];
    for (case, selectors, expected) in cases {
        let mut filtered: Vec<_> = all_tasks
            .iter()
            .filter(|task| {
                selectors
                    .iter()
                    .any(|selector| task_matches_filter(task.as_ref(), selector))
            })
            .map(|task| task.name())
            .collect();
        filtered.sort_unstable();
        assert_eq!(filtered, *expected, "{case}");
    }
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

#[test]
fn install_tasks_assess_on_linux_and_windows() {
    let platforms = [
        Platform {
            os: Os::Linux,
            is_arch: false,
            is_wsl: false,
        },
        Platform {
            os: Os::Windows,
            is_arch: false,
            is_wsl: false,
        },
    ];

    for platform in platforms {
        let ctx = common::TestContextBuilder::new().build();
        let ec = ctx.make_system_context(
            "base",
            platform,
            tasks::ContextOpts {
                dry_run: true,
                parallel: false,
                is_ci: None,
            },
        );

        for task in tasks::all_install_tasks(&ec.store) {
            let _ = task.should_run(&ec.ctx);
        }
    }
}

// ---------------------------------------------------------------------------
// Expected task presence
// ---------------------------------------------------------------------------

#[test]
fn install_task_catalog_contains_required_tasks() {
    let tasks = install_tasks();
    let selectors: Vec<&str> = tasks.iter().map(|task| task.selector()).collect();
    for required in ["symlinks", "git-hooks", "git", "system-files"] {
        assert!(
            selectors.contains(&required),
            "install task catalog is missing required selector '{required}'"
        );
    }
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

    use test_api::tasks::Task;

    let ctx = common::TestContextBuilder::new()
        .with_config_file(
            "chmod.toml",
            "[base]\npermissions = [{ mode = \"600\", path = \"ssh/config\" }]\n",
        )
        .build();

    let platform = Platform {
        os: Os::Linux,
        is_arch: false,
        is_wsl: false,
    };
    let ec = ctx.make_system_context(
        "base",
        platform,
        tasks::ContextOpts {
            dry_run: false,
            parallel: false,
            is_ci: Some(false),
        },
    );

    // Create $HOME/.ssh/config with mode 0o644.
    let ssh_dir = ec.ctx.home().join(".ssh");
    std::fs::create_dir_all(&ssh_dir).expect("create .ssh dir");
    let ssh_config = ssh_dir.join("config");
    std::fs::write(&ssh_config, "").expect("create ssh config");
    std::fs::set_permissions(&ssh_config, std::fs::Permissions::from_mode(0o644))
        .expect("set initial permissions");

    let result = tasks::files::chmod::ApplyFilePermissions::new(ec.store.chmod.clone())
        .run(&ec.ctx)
        .expect("apply file permissions run");
    assert!(
        matches!(result, tasks::TaskResult::Batch(ref stats) if stats.changed_count() > 0),
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

#[test]
fn install_run_dry_run_accepts_valid_filters() {
    let cases: &[(&str, &[&str], &[&str], bool)] = &[
        ("all tasks", &[], &[], false),
        ("only symlinks", &[], &["symlinks"], false),
        ("skip packages", &["packages"], &[], false),
        // Repository updates are already disabled by the helper.
        ("redundant repository skip", &["repository"], &[], false),
        ("parallel symlinks", &[], &["symlinks"], true),
    ];
    for (case, skip, only, parallel) in cases {
        let result = common::run_install_dry_run(
            skip.iter().map(|selector| (*selector).to_owned()).collect(),
            only.iter().map(|selector| (*selector).to_owned()).collect(),
            *parallel,
        );
        assert!(result.is_ok(), "{case}: {result:?}");
    }
}

/// Calling `install::run` with `--only` matching no selector must explain how
/// to discover valid selectors.
#[test]
fn install_run_dry_run_with_only_no_match_returns_an_actionable_error() {
    let result = common::run_install_dry_run(vec![], vec!["zzznomatch".to_string()], false);
    let error = result.expect_err("an unknown selector should fail");
    let message = error.to_string();
    assert!(message.contains("--only did not match a task selector"));
    assert!(message.contains("dotfiles tasks"));
}

/// Calling `install::run` with contradictory `--skip` and `--only` selectors
/// must fail instead of reporting a successful no-op.
#[test]
fn install_run_rejects_filters_that_select_no_tasks() {
    let result = common::run_install_dry_run(
        vec!["symlinks".to_string()],
        vec!["symlinks".to_string()],
        false,
    );
    let error = result.expect_err("contradictory task filters should fail");
    assert!(error.to_string().contains("selected no tasks"));
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
    let ctx = common::TestContextBuilder::new().build();
    let ec = ctx.make_system_context(
        "base",
        Platform::detect(),
        tasks::ContextOpts {
            dry_run: true,
            parallel: true,
            is_ci: Some(false),
        },
    );

    let all_tasks = install_tasks();
    for task in &all_tasks {
        let _ = task.should_run(&ec.ctx);
    }
}

#[test]
fn retained_history_selects_exact_runs_and_preserves_parent_and_actions() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "[base]\nsymlinks = [\"log-example\"]\n")
        .with_symlink_source_content("log-example", "example\n")
        .build();
    let home = tempfile::tempdir().unwrap();
    let overlay = tempfile::tempdir().unwrap();
    let log_dir = home.path().join("logs");
    let mut command = common::cli_command(
        repo.root_path(),
        home.path(),
        Some(overlay.path()),
        "install",
        "symlinks",
    );
    command.arg("--dry-run");
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parent = std::fs::read_dir(&log_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let parent_id = parent.file_stem().unwrap().to_str().unwrap();
    let parent_contents = std::fs::read_to_string(&parent).unwrap();
    let records: Vec<serde_json::Value> = parent_contents
        .lines()
        .filter_map(|line| {
            let (_, json) = line.split_once(" [record] ")?;
            serde_json::from_str(json).ok()
        })
        .collect();
    assert!(
        records
            .iter()
            .any(|r| r["type"] == "run_start" && r["run_id"] == parent_id)
    );
    assert!(
        records.iter().any(|r| r["type"] == "run_finish"
            && r["outcome"] == "succeeded"
            && r["exit_code"] == 0)
    );
    assert!(records.iter().any(|r| r["type"] == "action"
        && r["planned"] == true
        && r["subject"].as_str().unwrap().contains("log-example")));
    let task_id = records
        .iter()
        .find(|r| r["type"] == "task_result" && r["status"] == "dry_run")
        .unwrap()["task_id"]
        .as_str()
        .unwrap();

    // The same binary accepts explicit parent linkage without changing the preview scope.
    command.args(["--parent-run-id", parent_id]);
    assert!(command.output().unwrap().status.success());
    let history = read_retained_log(&log_dir, &["--list"]);
    assert!(
        history.contains("succeeded")
            && history.contains("base")
            && history.contains(&format!("/ {parent_id}")),
        "{history}"
    );
    assert_eq!(
        read_retained_log(&log_dir, &["--id", parent_id, "--raw"]),
        parent_contents
    );
    let task_log = read_retained_log(&log_dir, &["--id", parent_id, "--task", task_id]);
    assert!(
        task_log.contains("log-example") && !task_log.contains("[run_start]"),
        "{task_log}"
    );
    assert_eq!(
        std::fs::read_dir(&log_dir).unwrap().count(),
        2,
        "viewing logs must not create a run"
    );
    assert!(
        !home.path().join("log-example").exists(),
        "preview must not install the link"
    );
}

fn read_retained_log(log_dir: &std::path::Path, args: &[&str]) -> String {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_dotfiles"))
        .arg("log")
        .args(args)
        .env("DOTFILES_LOG_DIR", log_dir)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

#[test]
fn startup_failure_writes_finished_history_and_an_exact_diagnostic_hint() {
    let repo = common::TestContextBuilder::new()
        .with_config_file("symlinks.toml", "this is invalid toml")
        .build();
    let home = tempfile::tempdir().unwrap();
    let overlay = tempfile::tempdir().unwrap();
    let log_dir = home.path().join("logs");
    let output = common::cli_command(
        repo.root_path(),
        home.path(),
        Some(overlay.path()),
        "install",
        "symlinks",
    )
    .arg("--dry-run")
    .output()
    .unwrap();
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let id = text
        .split_once("Run 'dotfiles log --id ")
        .unwrap()
        .1
        .split_once(" -v'")
        .unwrap()
        .0;
    let raw = read_retained_log(&log_dir, &["--id", id, "--raw"]);
    assert!(
        raw.contains("\"type\":\"run_finish\"") && raw.contains("\"outcome\":\"failed\""),
        "{raw}"
    );
    assert!(read_retained_log(&log_dir, &["--list"]).contains("failed"));
}
