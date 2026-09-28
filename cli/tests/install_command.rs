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

#[test]
fn nested_cli_fixture_preserves_ancestor_settings_and_run_lock() {
    use std::io::{Read as _, Seek as _, Write as _};

    let ancestor = common::TestContextBuilder::new()
        .with_git_hooks_dir()
        .build();
    let repository = git2::Repository::open(ancestor.root_path()).unwrap();
    let mut settings = repository.config().unwrap();
    settings.set_str("dotfiles.profile", "desktop").unwrap();
    settings
        .set_str("dotfiles.overlay", "ancestor-overlay-sentinel")
        .unwrap();
    drop(settings);
    let config_path = repository.path().join("config");
    let original_config = std::fs::read(&config_path).unwrap();
    let lock_path = repository.path().join("dotfiles-run.lock");
    let original_lock = b"ancestor run-lock sentinel\n";
    let mut lock = std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    lock.write_all(original_lock).unwrap();
    lock.sync_all().unwrap();

    for locked in [false, true] {
        if locked {
            lock.try_lock().expect("hold the ancestor's run lock");
        }
        let root = ancestor.root_path().join(format!("nested-{locked}"));
        common::setup_minimal_repo(&root);
        assert_eq!(
            dunce::canonicalize(git2::Repository::discover(&root).unwrap().path()).unwrap(),
            dunce::canonicalize(repository.path()).unwrap(),
            "before isolation, this fixture must discover the ancestor"
        );
        let home = tempfile::tempdir().unwrap();
        let overlay = tempfile::tempdir().unwrap();
        let output = common::cli_command(
            &root,
            home.path(),
            Some(overlay.path()),
            "install",
            "completions",
        )
        .output()
        .unwrap();
        assert!(
            output.status.success(),
            "ancestor_locked={locked}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let child = git2::Repository::open(&root).unwrap();
        let child_settings = child.config().unwrap();
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("profile base"),
            "the child must use its requested profile rather than the ancestor's desktop selection"
        );
        assert_eq!(
            child_settings.get_string("dotfiles.overlay").unwrap(),
            overlay.path().to_string_lossy(),
            "the explicit overlay must be persisted only in the child"
        );
        assert!(child.path().join("dotfiles-run.lock").is_file());
        assert_eq!(
            std::fs::read(&config_path).unwrap(),
            original_config,
            "ancestor_locked={locked}: ancestor settings must remain byte-for-byte unchanged"
        );
        lock.rewind().unwrap();
        assert_eq!(
            lock.metadata().unwrap().len(),
            u64::try_from(original_lock.len()).unwrap(),
            "ancestor_locked={locked}: ancestor lock length must not change"
        );
        let mut contents = vec![0; original_lock.len()];
        lock.read_exact(&mut contents).unwrap();
        assert_eq!(
            contents, original_lock,
            "ancestor_locked={locked}: ancestor lock must not be overwritten"
        );
        if locked {
            lock.unlock().unwrap();
        }
    }
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
    let cases: &[(&str, &str, &[&str])] = &[
        ("single selector", "symlinks", &["Git hooks"]),
        ("multiple selectors", "symlinks,completions", &["Git hooks"]),
        (
            "outside selected subset",
            "registry",
            &["Git hooks", "Home symlinks"],
        ),
    ];
    for parallel in [false, true] {
        for (case, skip, expected) in cases {
            let repo = common::TestContextBuilder::new()
                .with_config_file("symlinks.toml", "[base]\nsymlinks = ['example']\n")
                .with_symlink_source("example")
                .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
                .with_git_hooks_dir()
                .build();
            let home = tempfile::tempdir().unwrap();
            let mut command = common::cli_command(
                repo.root_path(),
                home.path(),
                None,
                "install",
                "symlinks,git-hooks",
            );
            command.args(["--dry-run", "--skip", skip, "--verbose", "--no-symbols"]);
            if !parallel {
                command.arg("--no-parallel");
            }
            let output = command.output().unwrap();
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(
                output.status.success(),
                "{case}, parallel={parallel}: {text}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut names: Vec<_> = text
                .lines()
                .filter_map(|line| line.strip_prefix("DRYRUN "))
                .map(|line| line.split_once(" · ").expect("task row duration").0)
                .collect();
            names.sort_unstable();
            assert_eq!(names, *expected, "{case}, parallel={parallel}: {text}");
            assert!(home.path().join(".example").symlink_metadata().is_err());
            assert!(!repo.root_path().join(".git/hooks/pre-commit").exists());
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
fn minimal_install_catalog_applicability_is_platform_specific_and_scheduler_independent() {
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
        for parallel in [false, true] {
            let ctx = common::TestContextBuilder::new().build();
            let ec = ctx.make_context_with_executor(
                "base",
                platform,
                tasks::ContextOpts {
                    dry_run: true,
                    parallel,
                    is_ci: Some(false),
                },
                std::sync::Arc::new(common::StubExecutor),
            );
            let mut applicable: Vec<_> = tasks::all_install_tasks(&ec.store)
                .into_iter()
                .filter(|task| task.should_run(&ec.ctx))
                .map(|task| task.selector().to_string())
                .collect();
            applicable.sort_unstable();
            let mut expected = vec![
                "agent-settings",
                "completions",
                "git",
                "launcher",
                "path",
                "symlinks",
            ];
            if platform.os == Os::Windows {
                expected.extend(["developer-mode", "registry"]);
            } else {
                expected.extend(["file-permissions", "shell"]);
            }
            expected.sort_unstable();
            assert_eq!(applicable, expected, "{platform:?}, parallel={parallel}");
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
// install::run: full dry-run pipeline
// ---------------------------------------------------------------------------

#[test]
fn install_run_dry_run_accepts_valid_filters() {
    let cases: &[(&str, &[&str], &[&str], bool)] = &[
        ("filesystem tasks", &[], &["symlinks", "git-hooks"], false),
        ("only symlinks", &[], &["symlinks"], false),
        (
            "skip packages",
            &["packages"],
            &["symlinks", "git-hooks"],
            false,
        ),
        // Repository updates are already disabled by the helper.
        (
            "redundant repository skip",
            &["repository"],
            &["symlinks"],
            false,
        ),
        ("parallel symlinks", &[], &["symlinks"], true),
    ];
    for (case, skip, only, parallel) in cases {
        let result = common::run_install_dry_run(skip, only, *parallel);
        assert!(result.is_ok(), "{case}: {result:?}");
    }
}

/// Calling `install::run` with `--only` matching no selector must explain how
/// to discover valid selectors.
#[test]
fn install_run_unknown_filters_return_actionable_errors() {
    let cases: &[(&str, &[&str], &[&str])] = &[
        ("--only", &[], &["zzznomatch"]),
        ("--skip", &["zzznomatch"], &["symlinks"]),
    ];
    for (flag, skip, only) in cases {
        let result = common::run_install_dry_run(skip, only, false);
        let error = result.expect_err("an unknown selector should fail");
        let message = error.to_string();
        assert!(
            message.contains(&format!("{flag} did not match a task selector")),
            "{message}"
        );
        assert!(message.contains("dotfiles tasks"), "{message}");
    }
}

/// Calling `install::run` with contradictory `--skip` and `--only` selectors
/// must fail instead of reporting a successful no-op.
#[test]
fn install_run_rejects_filters_that_select_no_tasks() {
    let result = common::run_install_dry_run(&["symlinks"], &["symlinks"], false);
    let error = result.expect_err("contradictory task filters should fail");
    assert!(error.to_string().contains("selected no tasks"));
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
            line.split_once(" [record] ").map(|(_, json)| {
                serde_json::from_str(json).expect("every structured record must be valid JSON")
            })
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
        home.path().join(".log-example").symlink_metadata().is_err(),
        "preview must not install the link"
    );
}

fn read_retained_log(log_dir: &std::path::Path, args: &[&str]) -> String {
    let home = log_dir.parent().unwrap();
    let result = common::cli_command(home, home, None, "log", "")
        .args(args)
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
