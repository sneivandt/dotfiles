#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::wildcard_imports,
    clippy::indexing_slicing,
    reason = "panicking allowed at this trust boundary"
)]
//! Integration tests for the `update` command.
//!
//! These tests exercise the full task list produced by [`all_update_tasks`],
//! the task-selector filtering applied by the `--skip` and `--only` CLI
//! flags, and the structural properties of the update dependency graph.

mod common;

use dotfiles_cli::testing as test_api;

use test_api::config::ConfigStore;
use test_api::platform::{Os, Platform};
use test_api::tasks;

/// Build an update task list backed by a store loaded from a minimal repo.
fn update_tasks() -> Vec<Box<dyn tasks::Task>> {
    update_tasks_for_platform(Platform::detect())
}

fn update_tasks_for_platform(platform: Platform) -> Vec<Box<dyn tasks::Task>> {
    let ctx = common::IntegrationTestContext::new();
    let store = ConfigStore::from_config(ctx.load_config_for_platform("base", platform));
    tasks::all_update_tasks(&store)
}

#[test]
fn unsafe_elevated_child_scope_is_rejected_before_startup_writes() {
    for (verb, selector, inherited_marker) in [
        ("update", "symlinks", false),
        ("update", "registry", false),
        ("remove", "symlinks", false),
        ("update", "launcher", true),
        ("check", "config-files", true),
    ] {
        let fixture = common::IntegrationTestContext::new();
        let home = tempfile::tempdir().unwrap();
        // If the startup guard regresses, configuration loading must still
        // prevent native registry or other task mutations in this fixture.
        std::fs::write(
            fixture.root_path().join("conf/symlinks.toml"),
            "invalid = [",
        )
        .unwrap();
        let mut command =
            common::cli_command(fixture.root_path(), home.path(), None, verb, selector);
        if inherited_marker {
            command.env("DOTFILES_ELEVATED_CHILD", "1");
        } else {
            command.arg("--elevated-child");
        }
        let repository = git2::Repository::open(fixture.root_path()).unwrap();
        repository
            .config()
            .unwrap()
            .set_str("dotfiles.profile", "original-profile")
            .unwrap();
        let config_path = repository.path().join("config");
        let original_config = std::fs::read(&config_path).unwrap();

        let result = command.output().unwrap();
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!result.status.success(), "{verb}/{selector}: {output}");
        assert!(
            output.contains("elevated children"),
            "{verb}/{selector}: {output}"
        );
        assert_eq!(
            std::fs::read(config_path).unwrap(),
            original_config,
            "rejected scope must not persist a different profile"
        );
        assert_eq!(
            std::fs::read_dir(home.path()).unwrap().count(),
            0,
            "rejected scope must not create user settings, logs, or run locks"
        );
    }
}

#[test]
fn shared_home_lock_blocks_other_checkouts_before_configuration_mutations() {
    use std::io::{Read as _, Seek as _};

    let first = common::IntegrationTestContext::new();
    let second = common::IntegrationTestContext::new();
    let home = tempfile::tempdir().unwrap();
    let launcher = home.path().join(".local/bin").join(if cfg!(windows) {
        "dotfiles.cmd"
    } else {
        "dotfiles"
    });
    let lock_path = if cfg!(windows) {
        home.path().join("AppData/Local/dotfiles/home-run.lock")
    } else {
        home.path().join(".local/state/dotfiles/home-run.lock")
    };
    std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    let mut lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .unwrap();
    lock.try_lock().unwrap();
    let contents = b"fixture owner";
    std::io::Write::write_all(&mut lock, contents).unwrap();

    for repo in [&first, &second] {
        for (verb, selector) in [
            ("update", "launcher"),
            ("remove", "launcher"),
            ("check", "config-files"),
        ] {
            let mut command =
                common::cli_command(repo.root_path(), home.path(), None, verb, selector);
            command.env("XDG_STATE_HOME", home.path().join("other-state"));
            command.env("LOCALAPPDATA", home.path().join("other-local"));
            let result = command.output().unwrap();
            let output = format!(
                "{}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            assert!(!result.status.success(), "{verb}: {output}");
            assert!(output.contains("another dotfiles run"), "{verb}: {output}");
            assert!(output.contains("home-run.lock"), "{verb}: {output}");
            assert!(
                !launcher.exists(),
                "a blocked command must not install the launcher"
            );
        }
    }
    lock.rewind().unwrap();
    assert_eq!(
        lock.metadata().unwrap().len(),
        u64::try_from(contents.len()).unwrap()
    );
    let mut owner = vec![0; contents.len()];
    lock.read_exact(&mut owner).unwrap();
    assert_eq!(owner, contents, "contenders must preserve lock metadata");
    drop(lock);
    let result = common::cli_command(second.root_path(), home.path(), None, "update", "launcher")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        launcher.is_file(),
        "retry must install the launcher after the lock is released"
    );
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
            "update",
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
            String::from_utf8_lossy(&output.stdout).contains("base profile"),
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
fn update_console_separates_tasks_and_keeps_no_op_compact() {
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
                "update",
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
                let output = command.output().expect("run isolated update");
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
                        (true, false) => "CHANGED",
                        (false, true) => "○",
                        (false, false) => "UNCHANGED",
                    };
                    for (block, name) in blocks[1..3]
                        .iter()
                        .zip(["Home symlinks", "File permissions"])
                    {
                        assert!(block.starts_with(&format!("{status} {name}")), "{text}");
                    }
                }
                let summary = if changed {
                    "Tasks · 2 changed"
                } else {
                    "Tasks · 2 unchanged"
                };
                assert!(blocks.last().unwrap().starts_with(summary), "{text}");
            }
        }
    }
}

#[test]
fn conflicting_overlay_desired_state_stops_update_before_selected_tasks_run() {
    let mut cases = vec![(
        "git-config.toml",
        "[base]\nsettings = [{ key = \"core.editor\", value = \"vim\" }]\n",
        "[base]\nsettings = [\
         { key = \"core.editor\", value = \"vim\" },\
         { key = \"CORE.EDITOR\", value = \"nano\" }]\n",
        "git.conflicting-values",
    )];
    if cfg!(windows) {
        cases.push((
            "registry.toml",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n",
            "[console]\npath = 'HKCU:\\Console'\n[console.values]\nFontSize = 14\n\
             [other]\npath = 'hkcu:\\console'\n[other.values]\nfontsize = 15\n",
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
                "update",
                "symlinks",
            );
            let git_config = git2::Repository::open(repo.root_path())
                .unwrap()
                .path()
                .join("config");
            let saved = std::fs::read(&git_config).unwrap();

            if dry_run {
                command.arg("--dry-run");
            }
            let output = command.output().expect("run isolated update");
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
            assert_eq!(
                std::fs::read(&git_config).unwrap(),
                saved,
                "a failed load must leave the saved overlay unchanged"
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
fn update_task_catalog_satisfies_structural_contract() {
    let tasks = update_tasks();
    common::assert_task_catalog_contract("update", &tasks);
}

// ---------------------------------------------------------------------------
// Task-selector filtering
// ---------------------------------------------------------------------------

#[test]
fn update_filters_preview_exactly_the_selected_tasks_without_mutation() {
    let cases: &[(&str, &str, &str, &[&str])] = &[
        ("single only", "symlinks", "", &["symlinks"]),
        (
            "only union",
            "symlinks,git-hooks",
            "",
            &["git-hooks", "symlinks"],
        ),
        (
            "single skip",
            "symlinks,git-hooks",
            "symlinks",
            &["git-hooks"],
        ),
        (
            "multiple skips",
            "symlinks,git-hooks",
            "symlinks,completions",
            &["git-hooks"],
        ),
        (
            "outside selected subset",
            "symlinks,git-hooks",
            "registry",
            &["git-hooks", "symlinks"],
        ),
        (
            "skip packages",
            "symlinks,git-hooks",
            "packages",
            &["git-hooks", "symlinks"],
        ),
        (
            "redundant repository skip",
            "symlinks",
            "repository",
            &["symlinks"],
        ),
        (
            "normalized display name",
            "HOME_SYMLINKS",
            "",
            &["symlinks"],
        ),
    ];
    for parallel in [false, true] {
        for (case, only, skip, expected) in cases {
            let repo = common::TestContextBuilder::new()
                .with_config_file("symlinks.toml", "[base]\nsymlinks = ['example']\n")
                .with_symlink_source_content("example", "source must not change\n")
                .with_hook_source("pre-commit", "#!/bin/sh\nexit 0\n")
                .with_git_hooks_dir()
                .build();
            let home = tempfile::tempdir().unwrap();
            let mut command =
                common::cli_command(repo.root_path(), home.path(), None, "update", only);
            command.args(["--dry-run", "--format", "jsonl"]);
            if !skip.is_empty() {
                command.args(["--skip", skip]);
            }
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
            assert_previewed_tasks(&text, expected, case, parallel);
            assert!(home.path().join(".example").symlink_metadata().is_err());
            assert!(!repo.root_path().join(".git/hooks/pre-commit").exists());
            assert_eq!(
                std::fs::read(repo.root_path().join("symlinks/example")).unwrap(),
                b"source must not change\n"
            );
            assert_eq!(
                std::fs::read(repo.root_path().join("hooks/pre-commit")).unwrap(),
                b"#!/bin/sh\nexit 0\n"
            );
        }
    }
}

#[track_caller]
fn assert_previewed_tasks(text: &str, expected: &[&str], case: &str, parallel: bool) {
    let records: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid JSON record"))
        .collect();
    let mut outcomes: Vec<_> = records
        .iter()
        .filter(|record| record["type"] == "task_result")
        .map(|record| {
            (
                record["selector"].as_str().expect("task selector"),
                record["status"].as_str().expect("task status"),
            )
        })
        .collect();
    outcomes.sort_unstable();
    let expected: Vec<_> = expected
        .iter()
        .map(|selector| (*selector, "dry_run"))
        .collect();
    assert_eq!(outcomes, expected, "{case}, parallel={parallel}: {text}");
}

// ---------------------------------------------------------------------------
// Dry-run: task list from a minimal repository
// ---------------------------------------------------------------------------

#[test]
fn minimal_update_catalog_applicability_is_platform_specific_and_scheduler_independent() {
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
            let mut applicable: Vec<_> = tasks::all_update_tasks(&ec.store)
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
                expected.push("login-shell");
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
fn update_task_catalog_contains_required_tasks() {
    let tasks = update_tasks();
    let selectors: Vec<&str> = tasks.iter().map(|task| task.selector()).collect();
    for required in ["repository", "symlinks", "git-hooks", "git", "system-files"] {
        assert!(
            selectors.contains(&required),
            "update task catalog is missing required selector '{required}'"
        );
    }
}

// ---------------------------------------------------------------------------
// update::run: full dry-run pipeline
// ---------------------------------------------------------------------------

/// Calling `update::run` with `--only` matching no selector must explain how
/// to discover valid selectors.
#[test]
fn update_run_unknown_filters_return_actionable_errors() {
    let cases: &[(&str, &[&str], &[&str])] = &[
        ("--only", &[], &["zzznomatch"]),
        ("--only", &[], &["symlinks", "zzznomatch"]),
        ("--only", &[], &["update"]),
        ("--only", &[], &["report"]),
        ("--skip", &["zzznomatch"], &["symlinks"]),
    ];
    for (flag, skip, only) in cases {
        let result = common::run_update_dry_run(skip, only, false);
        let error = result.expect_err("an unknown selector should fail");
        let message = error.to_string();
        assert!(
            message.contains(&format!("{flag} did not match a task selector")),
            "{message}"
        );
        assert!(message.contains("dotfiles list"), "{message}");
    }
}

/// Calling `update::run` with contradictory `--skip` and `--only` selectors
/// must fail instead of reporting a successful no-op.
#[test]
fn update_run_rejects_filters_that_select_no_tasks() {
    let result = common::run_update_dry_run(&["symlinks"], &["symlinks"], false);
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
        "update",
        "symlinks",
    );
    command.arg("--dry-run");
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let logs = common::retained_logs(&log_dir);
    assert_eq!(logs.len(), 1);
    let parent = logs.first().unwrap();
    let parent_id = parent.file_stem().unwrap().to_str().unwrap();
    let parent_contents = std::fs::read_to_string(parent).unwrap();
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
    assert!(task_log.contains("Home symlinks · Planned:"), "{task_log}");
    let normal = read_retained_log(&log_dir, &["--id", parent_id]);
    for internal in [task_id, "[main]", "elapsed_us", "[info]"] {
        assert!(!normal.contains(internal), "{normal}");
    }
    assert_eq!(
        common::retained_logs(&log_dir).len(),
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
        "update",
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

#[cfg(unix)]
#[test]
fn repository_update_uses_each_fetch_result_when_a_later_fetch_overwrites_fetch_head() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let root = fixture.path().join("main");
    let overlay = fixture.path().join("overlay");
    let bin = fixture.path().join("bin");
    for dir in [&home, &overlay, &bin] {
        std::fs::create_dir(dir).unwrap();
    }
    common::setup_minimal_repo(&root);

    let mut expected = Vec::new();
    for (repo, name) in [(&root, "main-remote"), (&overlay, "overlay-remote")] {
        git(fixture.path(), &["init", "--bare", "-b", "main", name]);
        git(repo, &["init", "-b", "main"]);
        git(repo, &["add", "."]);
        git(repo, &["commit", "--allow-empty", "-m", "initial"]);
        let initial = git(repo, &["rev-parse", "HEAD"]);
        let remote = fixture.path().join(name);
        git(repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(repo, &["push", "-u", "origin", "main"]);
        git(repo, &["commit", "--allow-empty", "-m", name]);
        let upstream = git(repo, &["rev-parse", "HEAD"]);
        git(repo, &["push", "origin", "main"]);
        git(repo, &["commit", "--allow-empty", "-m", "unrelated"]);
        let unrelated = git(repo, &["rev-parse", "HEAD"]);
        git(repo, &["push", "origin", "HEAD:refs/heads/unrelated"]);
        git(repo, &["reset", "--hard", &initial]);
        expected.push((repo, upstream, unrelated));
    }

    let path = std::env::var_os("PATH").unwrap();
    let real_git = std::env::split_paths(&path)
        .find_map(|dir| dir.join("git").canonicalize().ok())
        .unwrap();
    let wrapped_git = bin.join("git");
    std::fs::write(
        &wrapped_git,
        r#"#!/bin/sh
if [ "$1" = fetch ]; then
    "$FIXTURE_GIT" "$@" || exit "$?"
    "$FIXTURE_GIT" fetch --quiet --no-tags origin refs/heads/unrelated
else
    exec "$FIXTURE_GIT" "$@"
fi
"#,
    )
    .unwrap();
    std::fs::set_permissions(wrapped_git, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path =
        std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&path))).unwrap();
    let output =
        common::isolated_cli_command(&root, &home, Some(&overlay), "update", "repository", true)
            .env("PATH", path)
            .env("FIXTURE_GIT", real_git)
            .output()
            .unwrap();
    assert!(output.status.success(), "{output:?}");
    for (repo, upstream, unrelated) in expected {
        assert_eq!(git(repo, &["rev-parse", "FETCH_HEAD"]), unrelated);
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), upstream);
        assert_eq!(git(repo, &["rev-parse", "@{u}"]), upstream);
    }
}

#[test]
fn partial_repository_refresh_can_retry_with_fresh_configuration() {
    check_partial_repository_refresh(false);
}

#[test]
fn ignored_overlay_file_blocks_merge_and_survives_retry() {
    check_partial_repository_refresh(true);
}

#[cfg(target_os = "linux")]
#[test]
fn failed_fast_forward_with_partial_write_stops_apply_until_checkout_is_repaired() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let repo = fixture.path().join("main");
    let publisher = fixture.path().join("publisher");
    let bin = fixture.path().join("bin");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&bin).unwrap();
    common::setup_minimal_repo(&repo);
    std::fs::write(
        repo.join("conf/symlinks.toml"),
        "[base]\nsymlinks=['app-settings']\n",
    )
    .unwrap();
    let source = "symlinks/app-settings";
    let original = br#"{"appearance":"dark"}"#;
    std::fs::write(repo.join(source), original).unwrap();
    let link = home.join(".app-settings");
    symlink(repo.join(source), &link).unwrap();

    git(fixture.path(), &["init", "--bare", "-b", "main", "origin"]);
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["remote", "add", "origin", "../origin"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "initial"]);
    git(&repo, &["push", "-u", "origin", "main"]);
    let initial = git(&repo, &["rev-parse", "HEAD"]);
    git(fixture.path(), &["clone", "origin", "publisher"]);
    let upstream = format!(
        r#"{{"appearance":"dark","longSetting":"{}"}}"#,
        "x".repeat(180_000)
    );
    std::fs::write(publisher.join(source), &upstream).unwrap();
    git(&publisher, &["commit", "-am", "expand settings"]);
    git(&publisher, &["push", "origin", "main"]);

    let path = std::env::var_os("PATH").unwrap();
    let real_git = std::env::split_paths(&path)
        .find_map(|dir| dir.join("git").canonicalize().ok())
        .unwrap();
    let wrapper = bin.join("git");
    std::fs::write(
        &wrapper,
        r#"#!/bin/bash
if [[ $1 == merge ]]; then
    trap '' XFSZ
    ulimit -f 4 || exit
fi
exec "$FIXTURE_GIT" "$@"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let wrapped_path =
        std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&path))).unwrap();
    let run = |tasks| common::isolated_cli_command(&repo, &home, None, "update", tasks, true);
    let failed = run("repository,symlinks,launcher")
        .env("PATH", wrapped_path)
        .env("FIXTURE_GIT", real_git)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&failed.stdout),
        String::from_utf8_lossy(&failed.stderr)
    );
    assert!(!failed.status.success(), "{text}");
    assert!(
        text.contains("the checkout changed during the failed merge"),
        "{text}"
    );
    assert!(
        text.contains("git status --short") && text.contains("git diff"),
        "{text}"
    );
    assert!(!home.join(".local/bin/dotfiles").exists(), "{text}");
    assert!(!text.contains("Home symlinks"), "{text}");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), initial);
    assert_eq!(std::fs::read_link(&link).unwrap(), repo.join(source));
    assert_eq!(std::fs::read(&link).unwrap(), upstream.as_bytes()[..4096]);

    let retry = run("repository").arg("--fail-on-skip").output().unwrap();
    assert!(!retry.status.success(), "{retry:?}");
    assert!(
        String::from_utf8_lossy(&retry.stdout).contains("local changes present"),
        "{retry:?}"
    );
    assert_eq!(std::fs::read(&link).unwrap(), upstream.as_bytes()[..4096]);

    git(&repo, &["restore", "--source=HEAD", "--", source]);
    assert_eq!(std::fs::read(&link).unwrap(), original);
    let recovered = run("repository,symlinks,launcher").output().unwrap();
    assert!(recovered.status.success(), "{recovered:?}");
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD"]),
        git(&publisher, &["rev-parse", "HEAD"])
    );
    assert_eq!(std::fs::read(&link).unwrap(), upstream.as_bytes());
    assert!(home.join(".local/bin/dotfiles").is_file());
}

#[cfg(unix)]
#[test]
fn repository_update_preserves_tracked_edits_made_during_fetch_with_autostash_enabled() {
    use std::os::unix::fs::PermissionsExt as _;

    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let root = fixture.path().join("main");
    let bin = fixture.path().join("bin");
    std::fs::create_dir(&home).unwrap();
    std::fs::create_dir(&bin).unwrap();
    common::setup_minimal_repo(&root);
    git(fixture.path(), &["init", "--bare", "-b", "main", "origin"]);
    git(&root, &["init", "-b", "main"]);
    git(&root, &["remote", "add", "origin", "../origin"]);
    std::fs::write(root.join("settings"), "initial\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "initial"]);
    git(&root, &["push", "-u", "origin", "main"]);
    let initial = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["config", "merge.autoStash", "true"]);
    git(fixture.path(), &["clone", "origin", "publisher"]);
    let publisher = fixture.path().join("publisher");
    std::fs::write(publisher.join("settings"), "upstream\n").unwrap();
    git(&publisher, &["commit", "-am", "change settings"]);
    git(&publisher, &["push", "origin", "main"]);

    let path = std::env::var_os("PATH").unwrap();
    let real_git = std::env::split_paths(&path)
        .find_map(|dir| dir.join("git").canonicalize().ok())
        .unwrap();
    let wrapped_git = bin.join("git");
    std::fs::write(
        &wrapped_git,
        r#"#!/bin/sh
if [ "$1" = fetch ]; then
    "$FIXTURE_GIT" "$@" || exit "$?"
    printf 'local edit after fetch\n' > settings
else
    exec "$FIXTURE_GIT" "$@"
fi
"#,
    )
    .unwrap();
    std::fs::set_permissions(wrapped_git, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path =
        std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&path))).unwrap();
    let output = common::isolated_cli_command(&root, &home, None, "update", "repository", true)
        .env("PATH", path)
        .env("FIXTURE_GIT", real_git)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("tracked files changed during update"),
        "{text}"
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), initial);
    assert_eq!(git(&root, &["stash", "list"]), "");
    assert_eq!(git(&root, &["ls-files", "--unmerged"]), "");
    assert_eq!(
        std::fs::read_to_string(root.join("settings")).unwrap(),
        "local edit after fetch\n"
    );
}

#[test]
fn restarted_overlay_update_keeps_a_validated_exclusion_when_its_script_is_retired() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("main");
    let overlay = fixture.path().join("overlay");
    let home = fixture.path().join("home");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&root);
    std::fs::create_dir_all(overlay.join("conf")).unwrap();
    for (repo, remote) in [(&root, "main-origin"), (&overlay, "overlay-origin")] {
        git(fixture.path(), &["init", "--bare", "-b", "main", remote]);
        git(repo, &["init", "-b", "main"]);
        let path = fixture.path().join(remote);
        git(repo, &["remote", "add", "origin", path.to_str().unwrap()]);
    }

    let overlay_scripts = overlay.join("conf/scripts.toml");
    std::fs::write(
        &overlay_scripts,
        "[base]\nscripts = [{ name = 'Retire fixture', path = 'retired.sh' }]\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        std::fs::create_dir(overlay.join("symlinks")).unwrap();
        std::fs::write(
            overlay.join("conf/symlinks.toml"),
            "[base]\nsymlinks = ['retired-source', 'foreign-replacement']\n",
        )
        .unwrap();
        for name in ["retired-source", "foreign-replacement"] {
            std::fs::write(overlay.join("symlinks").join(name), "managed\n").unwrap();
        }
        std::os::unix::fs::symlink(
            overlay.join("symlinks/retired-source"),
            home.join(".retired-source"),
        )
        .unwrap();
        std::fs::write(home.join("foreign-source"), "personal\n").unwrap();
        std::os::unix::fs::symlink("foreign-source", home.join(".foreign-replacement")).unwrap();
    }
    for repo in [&root, &overlay] {
        git(repo, &["add", "."]);
        git(repo, &["commit", "-m", "initial"]);
        git(repo, &["push", "-u", "origin", "main"]);
    }
    let initial = git(&overlay, &["rev-parse", "HEAD"]);
    // Publish the retirement from a separate clone so the tested checkout remains clean.
    git(fixture.path(), &["clone", "overlay-origin", "publisher"]);
    let publisher = fixture.path().join("publisher");
    std::fs::write(publisher.join("conf/scripts.toml"), "").unwrap();
    #[cfg(unix)]
    git(
        &publisher,
        &[
            "rm",
            "conf/symlinks.toml",
            "symlinks/retired-source",
            "symlinks/foreign-replacement",
        ],
    );
    git(&publisher, &["commit", "-am", "retire fixture script"]);
    git(&publisher, &["push", "origin", "main"]);

    let output = common::isolated_cli_command(
        &root,
        &home,
        Some(&overlay),
        "update",
        "repository,launcher",
        true,
    )
    .args(["--skip", "script-retire-fixture"])
    .output()
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_ne!(git(&overlay, &["rev-parse", "HEAD"]), initial);
    #[cfg(unix)]
    {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(text.contains("points to a missing source"), "{text}");
        assert!(!text.contains("foreign-replacement"), "{text}");
        assert_eq!(
            std::fs::read_link(home.join(".retired-source")).unwrap(),
            overlay.join("symlinks/retired-source"),
        );
        assert_eq!(
            std::fs::read_to_string(home.join(".foreign-replacement")).unwrap(),
            "personal\n",
        );
    }
    let launcher = if cfg!(windows) {
        "dotfiles.cmd"
    } else {
        "dotfiles"
    };
    assert!(home.join(".local/bin").join(launcher).is_file());
}

#[allow(
    clippy::too_many_lines,
    reason = "real Git remotes and update recovery"
)]
fn check_partial_repository_refresh(ignored: bool) {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let root = fixture.path().join("main");
    let overlay = fixture.path().join("overlay");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&root);
    std::fs::create_dir(&overlay).unwrap();
    for (repo, remote_name) in [(&root, "main-upstream"), (&overlay, "overlay-upstream")] {
        git(
            fixture.path(),
            &["init", "--bare", "-b", "main", remote_name],
        );
        git(repo, &["init", "-b", "main"]);
        std::fs::write(repo.join("README"), "initial").unwrap();
        if ignored && repo == &overlay {
            std::fs::write(repo.join(".gitignore"), "collision\n").unwrap();
        }
        git(repo, &["add", "."]);
        git(repo, &["commit", "-m", "initial"]);
        let remote = fixture.path().join(remote_name);
        git(repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(repo, &["push", "-u", "origin", "main"]);
    }
    let initial_main = git(&root, &["rev-parse", "HEAD"]);
    let initial_overlay = git(&overlay, &["rev-parse", "HEAD"]);
    std::fs::write(
        root.join("conf/symlinks.toml"),
        "[base]\nsymlinks = [\"fresh\"]\n",
    )
    .unwrap();
    std::fs::write(root.join("symlinks/fresh"), "new configuration").unwrap();
    std::fs::write(overlay.join("collision"), "upstream").unwrap();
    git(&overlay, &["add", "-f", "collision"]);
    std::fs::create_dir(overlay.join("conf")).unwrap();
    std::fs::create_dir(overlay.join("symlinks")).unwrap();
    std::fs::write(
        overlay.join("conf/symlinks.toml"),
        "[base]\nsymlinks = [\"second\"]\n",
    )
    .unwrap();
    std::fs::write(overlay.join("symlinks/second"), "fresh overlay").unwrap();
    for (repo, initial) in [(&root, &initial_main), (&overlay, &initial_overlay)] {
        git(repo, &["add", "."]);
        git(repo, &["commit", "-m", "advance"]);
        git(repo, &["push", "origin", "main"]);
        git(repo, &["reset", "--hard", initial]);
    }
    let obstruction = overlay.join("collision");
    std::fs::write(&obstruction, "local untracked data").unwrap();

    if ignored {
        let preview = common::isolated_cli_command(
            &root,
            &home,
            Some(&overlay),
            "update",
            "repository",
            true,
        )
        .arg("--dry-run")
        .output()
        .unwrap();
        assert!(preview.status.success(), "{preview:?}");
        assert!(
            String::from_utf8_lossy(&preview.stdout)
                .contains("local merge eligibility not checked"),
            "preview must disclose the unchecked local collision: {preview:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&obstruction).unwrap(),
            "local untracked data",
            "preview must preserve the ignored file"
        );
    }

    let update = || {
        let output = common::isolated_cli_command(
            &root,
            &home,
            Some(&overlay),
            "update",
            "repository,symlinks",
            true,
        )
        .output()
        .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        (output.status, text)
    };
    let (status, text) = update();
    assert!(!status.success(), "{text}");
    assert!(text.contains("Already advanced repository"), "{text}");
    assert_ne!(
        git(&root, &["rev-parse", "HEAD"]),
        initial_main,
        "the main checkout should advance before the overlay blocks"
    );
    assert_eq!(
        git(&overlay, &["rev-parse", "HEAD"]),
        initial_overlay,
        "the blocked overlay should keep its original HEAD"
    );
    assert_eq!(
        std::fs::read_to_string(&obstruction).unwrap(),
        "local untracked data",
        "the overlay merge must preserve the local obstruction"
    );
    assert!(
        !home.join(".fresh").exists(),
        "convergence must wait for repository sync"
    );

    std::fs::remove_file(obstruction).unwrap();
    let (retry_status, retry_text) = update();
    assert!(retry_status.success(), "{retry_text}");
    assert_ne!(
        git(&overlay, &["rev-parse", "HEAD"]),
        initial_overlay,
        "the overlay should advance after the obstruction is removed"
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".second")).unwrap(),
        "fresh overlay",
        "the retry must apply the refreshed overlay configuration"
    );
    assert_eq!(
        std::fs::read_to_string(home.join(".fresh")).unwrap(),
        "new configuration",
        "the retry must apply the refreshed main configuration"
    );

    git(&root, &["commit", "--allow-empty", "-m", "local only"]);
    let local_head = git(&root, &["rev-parse", "HEAD"]);
    let check_status = |expected: &str, dry_run| {
        let mut command =
            common::isolated_cli_command(&root, &home, None, "update", "repository", true);
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        let status_text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if dry_run {
            assert!(output.status.success(), "{status_text}");
        }
        assert!(!status_text.contains("would change"), "{status_text}");
        assert!(status_text.contains(expected), "{status_text}");
        assert_eq!(git(&root, &["rev-parse", "HEAD"]), local_head);
    };
    check_status("local branch is ahead of upstream", true);
    check_status("local branch is ahead of upstream", false);

    let publisher = fixture.path().join("publisher");
    git(
        fixture.path(),
        &["clone", "--no-local", "main-upstream", "publisher"],
    );
    git(
        &publisher,
        &["commit", "--allow-empty", "-m", "upstream only"],
    );
    git(&publisher, &["push", "origin", "main"]);
    let tracking_before = git(&root, &["rev-parse", "@{u}"]);
    check_status("cannot determine fast-forward eligibility", true);
    assert_eq!(
        git(&root, &["rev-parse", "@{u}"]),
        tracking_before,
        "preview must not update the local tracking ref"
    );

    git(&root, &["fetch", "origin"]);
    check_status("local branch diverged from upstream", true);
    check_status("local branch diverged from upstream", false);
}

#[test]
fn repository_update_isolates_checkout_and_honors_system_git_configuration() {
    let fixture = tempfile::tempdir().unwrap();
    let home = fixture.path().join("home");
    let root = fixture.path().join("repo");
    let upstream = fixture.path().join("upstream");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&root);
    git(&root, &["init", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "initial"]);
    let initial = git(&root, &["rev-parse", "HEAD"]);
    git(
        fixture.path(),
        &["clone", root.to_str().unwrap(), upstream.to_str().unwrap()],
    );
    git(
        &root,
        &["remote", "add", "origin", upstream.to_str().unwrap()],
    );
    git(&root, &["fetch", "origin"]);
    git(&root, &["branch", "--set-upstream-to=origin/main"]);
    // Publish only after establishing the old tracking ref so the first apply
    // must parse a real fast-forward line with a blank porcelain status.
    std::fs::write(upstream.join("new-file"), "upstream content").unwrap();
    git(&upstream, &["add", "new-file"]);
    git(&upstream, &["commit", "-m", "advance"]);
    let expected = git(&upstream, &["rev-parse", "HEAD"]);
    git(
        &root,
        &["remote", "set-url", "origin", "audit-system://upstream"],
    );
    let system_config = fixture.path().join("system.gitconfig");
    git(
        &root,
        &[
            "config",
            "--file",
            system_config.to_str().unwrap(),
            &format!("url.{}.insteadOf", upstream.to_str().unwrap()),
            "audit-system://upstream",
        ],
    );
    let unrelated = fixture.path().join("unrelated");
    git(
        fixture.path(),
        &[
            "clone",
            "--branch",
            "main",
            root.to_str().unwrap(),
            "unrelated",
        ],
    );
    let unrelated_index = unrelated.join(".git/index");
    let original_index = std::fs::read(&unrelated_index).unwrap();
    for dry_run in [true, false] {
        let mut command =
            common::isolated_cli_command(&root, &home, None, "update", "repository", true);
        command
            .env("GIT_CONFIG_SYSTEM", &system_config)
            .env("GIT_CONFIG_NOSYSTEM", "0")
            .env("GIT_DIR", unrelated.join(".git"))
            .env("GIT_COMMON_DIR", unrelated.join(".git"))
            .env("GIT_WORK_TREE", &unrelated)
            .env("GIT_INDEX_FILE", &unrelated_index)
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "branch.main.remote")
            .env("GIT_CONFIG_VALUE_0", "wrong-remote");
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "dry_run={dry_run}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            git(&root, &["rev-parse", "HEAD"]),
            if dry_run {
                initial.as_str()
            } else {
                expected.as_str()
            }
        );
        assert_eq!(
            git(&root, &["rev-parse", "@{u}"]),
            if dry_run {
                initial.as_str()
            } else {
                expected.as_str()
            },
            "preview preserves tracking state and the first apply advances it"
        );
        assert_eq!(git(&unrelated, &["rev-parse", "HEAD"]), initial);
        assert_eq!(std::fs::read(&unrelated_index).unwrap(), original_index);
        assert!(!unrelated.join(".git/FETCH_HEAD").exists());
        assert!(!unrelated.join("new-file").exists());
    }
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(key);
        }
    }
    let output = command
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@test.local",
        ])
        .args([
            "-c",
            "core.hooksPath=unused-hooks",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", root.join("no-global-config"))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn git_fixture_ignores_inherited_configuration_and_repository_paths() {
    const CHILD: &str = "DOTFILES_TEST_GIT_FIXTURE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "--bare", "-b", "main"]);
        assert_eq!(
            git(root.path(), &["rev-parse", "--is-bare-repository"]),
            "true"
        );
        assert_eq!(
            std::fs::canonicalize(git(root.path(), &["rev-parse", "--absolute-git-dir"])).unwrap(),
            std::fs::canonicalize(root.path()).unwrap()
        );
        return;
    }
    let unrelated = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "git_fixture_ignores_inherited_configuration_and_repository_paths",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "safe.bareRepository")
        .env("GIT_CONFIG_VALUE_0", "explicit")
        .env("GIT_DIR", unrelated.path().join("not-a-repository"))
        .env("GIT_WORK_TREE", unrelated.path())
        .env("GIT_INDEX_FILE", unrelated.path().join("index"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(std::fs::read_dir(unrelated.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn repository_restart_preserves_non_unicode_paths_and_loads_new_configuration() {
    use std::os::unix::ffi::OsStringExt as _;
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture
        .path()
        .join(std::ffi::OsString::from_vec(b"checkout-\xff".to_vec()));
    let home = fixture.path().join("home");
    let remote = fixture.path().join("upstream");
    std::fs::create_dir(&home).unwrap();
    common::setup_minimal_repo(&root);
    git(
        fixture.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "initial"]);
    let initial = git(&root, &["rev-parse", "HEAD"]);
    git(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&root, &["push", "-u", "origin", "main"]);
    std::fs::write(
        root.join("conf/symlinks.toml"),
        "[base]\nsymlinks = [\"fresh\"]\n",
    )
    .unwrap();
    std::fs::write(root.join("symlinks/fresh"), "new configuration\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "new configuration"]);
    let updated = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["push", "origin", "main"]);
    git(&root, &["reset", "--hard", &initial]);
    let run_update = || {
        common::isolated_cli_command(&root, &home, None, "update", "repository,symlinks", true)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    };
    let result = run_update();
    assert!(result.status.success(), "{result:?}");
    let stdout = String::from_utf8(result.stdout).unwrap();
    let after_restart = stdout
        .split_once("Repository synced · restarting to load configuration\n")
        .expect("expected the repository restart notice")
        .1;
    assert!(
        after_restart.starts_with('\n') && !after_restart.starts_with("\n\n"),
        "expected one separator after the restart notice in {stdout:?}"
    );
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), updated);
    assert_eq!(
        std::fs::read_to_string(home.join(".fresh")).unwrap(),
        "new configuration\n"
    );
    assert_eq!(
        std::fs::read_link(home.join(".fresh")).unwrap(),
        root.join("symlinks/fresh")
    );
    let logs = common::retained_logs(&home.join("logs"));
    assert_eq!(logs.len(), 2, "restart must create a linked child run");
    let child = logs
        .iter()
        .map(|log| std::fs::read_to_string(log).unwrap())
        .any(|log| log.contains("\"parent_run_id\":\""));
    assert!(child, "restart must retain parent-run linkage");

    // Repeat the fast-forward with the managed link already in place.
    git(&root, &["reset", "--hard", &initial]);
    let repeat = run_update();
    assert!(repeat.status.success(), "{repeat:?}");
    let repeat_stdout = String::from_utf8(repeat.stdout).unwrap();
    assert!(
        repeat_stdout.contains(
            "Repository synced · restarting to load configuration\n\nTasks · 1 unchanged ·"
        ),
        "expected the no-change summary after one separator in {repeat_stdout:?}"
    );
}

#[test]
fn repository_only_update_recovers_bad_config_and_refuses_a_deleted_upstream() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("checkout");
    let home = fixture.path().join("home");
    let remote = fixture.path().join("remote");
    common::setup_minimal_repo(&root);
    std::fs::create_dir(&home).unwrap();
    let config = root.join("conf/symlinks.toml");
    std::fs::write(&config, "[invalid").unwrap();
    git(
        fixture.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(&root, &["init", "-b", "main"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "broken configuration"]);
    let broken = git(&root, &["rev-parse", "HEAD"]);
    git(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&root, &["push", "-u", "origin", "main"]);
    std::fs::write(&config, "").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "repair configuration"]);
    let fixed = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["push", "origin", "main"]);
    git(&root, &["reset", "--hard", &broken]);
    git(&root, &["config", "branch.main.mergeOptions", "--squash"]);

    let overlay = tempfile::tempdir().unwrap();
    let update = |selector| {
        common::isolated_cli_command(&root, &home, Some(overlay.path()), "update", selector, true)
    };
    let config_file = std::fs::File::options().write(true).open(&config).unwrap();
    config_file
        .set_times(std::fs::FileTimes::new().set_modified(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000),
        ))
        .unwrap();
    drop(config_file);
    let index = root.join(".git/index");
    let index_before = std::fs::read(&index).unwrap();
    let preview = update("repository").arg("--dry-run").output().unwrap();
    assert!(preview.status.success(), "{preview:?}");
    let settings = git2::Repository::open(&root).unwrap().config().unwrap();
    assert_eq!(
        settings.get_string("dotfiles.overlay").unwrap(),
        overlay.path().to_string_lossy(),
        "repository-only preview saves the explicit overlay despite invalid aggregate configuration"
    );
    assert_eq!(
        std::fs::read(&index).unwrap(),
        index_before,
        "repository preview must leave the Git index unchanged"
    );
    assert!(
        String::from_utf8_lossy(&preview.stdout).contains("local merge eligibility not checked"),
        "repository-only preview must reach Git despite the invalid config: {preview:?}"
    );
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        broken,
        "this operation must leave HEAD at its original commit"
    );
    let repair = update("Dotfiles repository").output().unwrap();
    assert!(repair.status.success(), "{repair:?}");
    assert!(
        String::from_utf8_lossy(&repair.stdout).contains("Tasks · 1 changed"),
        "repository-only repair must report the change: {repair:?}"
    );
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        fixed,
        "must fast-forward to the repair commit even with squash configured"
    );
    let status = git(&root, &["status", "--porcelain", "-uno"]);
    assert!(status.is_empty(), "expected a clean checkout: {status}");
    let content = std::fs::read(&config).unwrap();
    assert!(content.is_empty(), "must restore the valid config");

    git(&root, &["reset", "--hard", &broken]);
    git(&remote, &["update-ref", "-d", "refs/heads/main"]);
    assert_eq!(
        git(&root, &["rev-parse", "@{u}"]),
        fixed,
        "the deleted branch must remain available through the stale tracking ref"
    );
    let missing = update("repository").output().unwrap();
    assert!(!missing.status.success(), "{missing:?}");
    assert_eq!(
        git(&root, &["rev-parse", "HEAD"]),
        broken,
        "this operation must leave HEAD at its original commit"
    );
}
